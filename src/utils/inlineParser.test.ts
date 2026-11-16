import { describe, expect, it } from 'vitest';
import {
  isRunnableCodeLang,
  parseLocalFileTarget,
  tokenizeInline,
} from './inlineParser';

describe('inlineParser', () => {
  it('parses file:/// links with spaces, Chinese characters, and #L line ranges', () => {
    const target = parseLocalFileTarget(
      'file:///D:/huangjunjie/fine-tune%20the%20model/%E4%B8%8E%E9%9B%B7%E8%80%81%E5%B8%88/notes.md#L15-L28'
    );
    expect(target).toEqual({
      path: 'D:\\huangjunjie\\fine-tune the model\\与雷老师\\notes.md',
      line: 15,
      endLine: 28,
    });
  });

  it('tokenizes markdown file links, conversation links, web links, bold, and code', () => {
    const line =
      '请查看 **[markdownRenderer.ts](file:///D:/tools/md-pdf-viewer/src/utils/markdownRenderer.ts#L10-L20)** 或 [前序会话](conversation://39eb79da-49da-4152-ab74-a5373256e6e3) 以及 `D:\\proj\\train.py:42` 和 https://example.com/docs。';
    const tokens = tokenizeInline(line);

    const conv = tokens.find((t) => t.kind === 'conv-link');
    expect(conv).toEqual({
      kind: 'conv-link',
      label: '前序会话',
      conversationId: '39eb79da-49da-4152-ab74-a5373256e6e3',
    });

    const codeToken = tokens.find((t) => t.kind === 'code');
    expect(codeToken).toEqual({
      kind: 'code',
      text: 'D:\\proj\\train.py:42',
      localFile: {
        path: 'D:\\proj\\train.py',
        line: 42,
      },
    });

    const web = tokens.find((t) => t.kind === 'web-link');
    expect(web).toEqual({
      kind: 'web-link',
      label: 'https://example.com/docs',
      url: 'https://example.com/docs',
    });
  });

  it('tokenizes standalone markdown file link cleanly', () => {
    const tokens = tokenizeInline(
      '参考 ([App.tsx](file:///D:/huangjunjie/tools/md-pdf-viewer/src/App.tsx#L120))'
    );
    const fileLink = tokens.find((t) => t.kind === 'file-link');
    expect(fileLink).toEqual({
      kind: 'file-link',
      label: 'App.tsx',
      path: 'D:\\huangjunjie\\tools\\md-pdf-viewer\\src\\App.tsx',
      line: 120,
      endLine: undefined,
    });
  });

  it('identifies runnable languages accurately', () => {
    expect(isRunnableCodeLang('python')).toBe(true);
    expect(isRunnableCodeLang('ps1')).toBe(true);
    expect(isRunnableCodeLang('json')).toBe(false);
  });
});
