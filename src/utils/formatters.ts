export function formatDateTime(isoString: string): string {
  if (!isoString) return '';
  const date = new Date(isoString);
  if (isNaN(date.getTime())) return isoString;

  const y = date.getFullYear();
  const m = String(date.getMonth() + 1).padStart(2, '0');
  const d = String(date.getDate()).padStart(2, '0');
  const hh = String(date.getHours()).padStart(2, '0');
  const mm = String(date.getMinutes()).padStart(2, '0');
  const ss = String(date.getSeconds()).padStart(2, '0');

  return `${y}-${m}-${d} ${hh}:${mm}:${ss}`;
}

export function extractProjectName(projectPath?: string): string {
  if (!projectPath) return '未分类会话';
  const norm = projectPath.replace(/\\/g, '/').replace(/\/+$/, '');
  const segments = norm.split('/');
  return segments[segments.length - 1] || '未分类会话';
}
