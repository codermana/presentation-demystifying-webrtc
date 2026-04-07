use std::{ptr, ptr::NonNull, slice, time::Duration};

use bytes::Bytes;
use objc2_core_media::{
    CMFormatDescription, CMSampleBuffer, CMVideoFormatDescriptionGetH264ParameterSetAtIndex,
};

#[derive(Clone, Debug)]
pub struct EncodedAccessUnit {
    pub data: Bytes,
    pub duration: Duration,
}

pub fn sample_buffer_to_access_unit(
    sample_buffer: &CMSampleBuffer,
    duration: Duration,
) -> Result<EncodedAccessUnit, String> {
    let format_description = unsafe { sample_buffer.format_description() }
        .ok_or_else(|| "encoded sample buffer missing format description".to_string())?;
    let data_buffer = unsafe { sample_buffer.data_buffer() }
        .ok_or_else(|| "encoded sample buffer missing block buffer".to_string())?;

    let is_keyframe = true;
    let header_len = nal_unit_header_length(format_description.as_ref())?;
    let mut avcc_bytes = vec![0_u8; unsafe { data_buffer.data_length() as usize }];
    let destination = NonNull::new(avcc_bytes.as_mut_ptr().cast())
        .ok_or_else(|| "null destination buffer".to_string())?;
    let status = unsafe { data_buffer.copy_data_bytes(0, avcc_bytes.len(), destination) };
    if status != 0 {
        return Err(format!("CMBlockBuffer::copy_data_bytes failed: {status}"));
    }

    let mut annex_b = Vec::with_capacity(avcc_bytes.len() + 128);
    if is_keyframe {
        append_parameter_sets(format_description.as_ref(), &mut annex_b)?;
    }
    append_avcc_nals_as_annex_b(&avcc_bytes, header_len, &mut annex_b)?;

    Ok(EncodedAccessUnit {
        data: Bytes::from(annex_b),
        duration,
    })
}

pub fn parse_annex_b_access_units(
    bytes: &[u8],
    duration: Duration,
) -> Result<Vec<EncodedAccessUnit>, String> {
    let nal_units = split_annex_b_nals(bytes);
    if nal_units.is_empty() {
        return Ok(Vec::new());
    }

    if nal_units.iter().any(|nal| nal_unit_type(nal) == Some(9)) {
        return parse_access_units_from_aud(&nal_units, duration);
    }

    parse_access_units_without_aud(&nal_units, duration)
}

fn nal_unit_header_length(format_description: &CMFormatDescription) -> Result<usize, String> {
    let mut nal_header_length = 0_i32;
    let status = unsafe {
        CMVideoFormatDescriptionGetH264ParameterSetAtIndex(
            format_description,
            0,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            &mut nal_header_length,
        )
    };
    if status != 0 {
        return Err(format!(
            "CMVideoFormatDescriptionGetH264ParameterSetAtIndex failed: {status}"
        ));
    }

    Ok(nal_header_length.max(1) as usize)
}

fn append_parameter_sets(
    format_description: &CMFormatDescription,
    output: &mut Vec<u8>,
) -> Result<(), String> {
    let mut parameter_set_count = 0_usize;
    let status = unsafe {
        CMVideoFormatDescriptionGetH264ParameterSetAtIndex(
            format_description,
            0,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut parameter_set_count,
            ptr::null_mut(),
        )
    };
    if status != 0 {
        return Err(format!(
            "CMVideoFormatDescriptionGetH264ParameterSetAtIndex count failed: {status}"
        ));
    }

    for index in 0..parameter_set_count {
        let mut parameter_ptr = ptr::null();
        let mut parameter_len = 0_usize;
        let status = unsafe {
            CMVideoFormatDescriptionGetH264ParameterSetAtIndex(
                format_description,
                index,
                &mut parameter_ptr,
                &mut parameter_len,
                ptr::null_mut(),
                ptr::null_mut(),
            )
        };
        if status != 0 {
            return Err(format!(
                "CMVideoFormatDescriptionGetH264ParameterSetAtIndex({index}) failed: {status}"
            ));
        }

        output.extend_from_slice(&[0, 0, 0, 1]);
        output.extend_from_slice(unsafe { slice::from_raw_parts(parameter_ptr, parameter_len) });
    }

    Ok(())
}

fn append_avcc_nals_as_annex_b(
    avcc_bytes: &[u8],
    header_len: usize,
    output: &mut Vec<u8>,
) -> Result<(), String> {
    let mut cursor = 0_usize;
    while cursor + header_len <= avcc_bytes.len() {
        let nal_len = parse_nal_length(&avcc_bytes[cursor..cursor + header_len]);
        cursor += header_len;
        if cursor + nal_len > avcc_bytes.len() {
            return Err("invalid H264 AVCC payload length".to_string());
        }

        output.extend_from_slice(&[0, 0, 0, 1]);
        output.extend_from_slice(&avcc_bytes[cursor..cursor + nal_len]);
        cursor += nal_len;
    }

    Ok(())
}

fn split_annex_b_nals(bytes: &[u8]) -> Vec<Vec<u8>> {
    let mut nal_units = Vec::new();
    let mut cursor = 0_usize;

    while let Some(start) = find_start_code(bytes, cursor) {
        let nal_start = start + start_code_len(bytes, start);
        let Some(next_start) = find_start_code(bytes, nal_start) else {
            if nal_start < bytes.len() {
                nal_units.push(bytes[nal_start..].to_vec());
            }
            break;
        };
        if nal_start < next_start {
            nal_units.push(bytes[nal_start..next_start].to_vec());
        }
        cursor = next_start;
    }

    nal_units
}

fn parse_access_units_from_aud(
    nal_units: &[Vec<u8>],
    duration: Duration,
) -> Result<Vec<EncodedAccessUnit>, String> {
    let mut access_units = Vec::new();
    let mut current = Vec::new();

    for nal in nal_units {
        if nal_unit_type(nal) == Some(9) {
            if !current.is_empty() {
                access_units.push(make_access_unit(&current, duration));
                current.clear();
            }
            continue;
        }
        current.push(nal.clone());
    }

    if !current.is_empty() {
        access_units.push(make_access_unit(&current, duration));
    }

    if access_units.is_empty() {
        return Err("Annex-B stream contained AUD markers but no video access units".to_string());
    }

    Ok(access_units)
}

fn parse_access_units_without_aud(
    nal_units: &[Vec<u8>],
    duration: Duration,
) -> Result<Vec<EncodedAccessUnit>, String> {
    let mut access_units = Vec::new();
    let mut parameter_sets = Vec::new();

    for nal in nal_units {
        match nal_unit_type(nal) {
            Some(7 | 8 | 6) => parameter_sets.push(nal.clone()),
            Some(5) => {
                let mut grouped = parameter_sets.clone();
                grouped.push(nal.clone());
                access_units.push(make_access_unit(&grouped, duration));
            }
            Some(1) => access_units.push(make_access_unit(std::slice::from_ref(nal), duration)),
            Some(_) | None => {}
        }
    }

    if access_units.is_empty() {
        return Err(
            "unable to derive access units from Annex-B stream; add AUD NALs or provide a simpler H264 elementary stream"
                .to_string(),
        );
    }

    Ok(access_units)
}

fn make_access_unit(nal_units: &[Vec<u8>], duration: Duration) -> EncodedAccessUnit {
    let mut data = Vec::new();
    for nal in nal_units {
        data.extend_from_slice(&[0, 0, 0, 1]);
        data.extend_from_slice(nal);
    }

    EncodedAccessUnit {
        data: data.into(),
        duration,
    }
}

fn find_start_code(bytes: &[u8], from: usize) -> Option<usize> {
    let mut cursor = from;
    while cursor + 3 < bytes.len() {
        if bytes[cursor..].starts_with(&[0, 0, 1]) || bytes[cursor..].starts_with(&[0, 0, 0, 1]) {
            return Some(cursor);
        }
        cursor += 1;
    }
    None
}

fn start_code_len(bytes: &[u8], start: usize) -> usize {
    if bytes[start..].starts_with(&[0, 0, 0, 1]) {
        4
    } else {
        3
    }
}

fn nal_unit_type(nal: &[u8]) -> Option<u8> {
    nal.first().map(|byte| byte & 0x1f)
}

fn parse_nal_length(bytes: &[u8]) -> usize {
    bytes
        .iter()
        .fold(0_usize, |acc, byte| (acc << 8) | usize::from(*byte))
}
