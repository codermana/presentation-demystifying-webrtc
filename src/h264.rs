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

fn parse_nal_length(bytes: &[u8]) -> usize {
    bytes
        .iter()
        .fold(0_usize, |acc, byte| (acc << 8) | usize::from(*byte))
}
