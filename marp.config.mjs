import { Marp } from '@marp-team/marp-core';
import { createHash } from 'node:crypto';

export default {
  engine: (opts) => {
    const marp = new Marp(opts);
    
    marp.use((md) => {
      const defaultRender = md.renderer.rules.fence;
      md.renderer.rules.fence = (tokens, idx, options, env, self) => {
        const token = tokens[idx];
        if (token.info.trim() === 'mermaid') {
          const code = token.content.trim();
          const hash = createHash('sha256').update(code).digest('hex').substring(0, 12);
          // Return an img tag instead of the code block
          return `<p class="mermaid-container" style="text-align: center;"><img src="assets/generated/mermaid/${hash}.svg" alt="Mermaid Diagram" style="width: 100%; height: 400px; object-fit: contain; display: block; margin: 0 auto;"></p>\n`;
        }
        return defaultRender(tokens, idx, options, env, self);
      };
    });
    
    return marp;
  }
};
