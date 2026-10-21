/** Match Windows folder identities across drive casing and extended path syntax. */
export function folderPinKey(path: string): string {
  return path.trim().replace(/^"|"$/g, '')
    .replace(/^\\\\\?\\UNC\\/i, '\\\\')
    .replace(/^\\\\\?\\/, '')
    .replace(/\//g, '\\').replace(/\\+$/, '').toLowerCase();
}
