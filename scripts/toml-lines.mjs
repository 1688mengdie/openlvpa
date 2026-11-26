
export function contentOf(line) {
  return line.endsWith('\r') ? line.slice(0, -1) : line;
}

export function lineTerminator(text) {
  return text.includes('\r\n') ? '\r' : '';
}

export function splitLines(text) {
  return text.split('\n');
}

export function tableLines(text, table) {
  const lines = splitLines(text).map(contentOf);
  const header = `[${table}]`;
  const start = lines.findIndex((line) => line.trim() === header);
  if (start === -1) {
    throw new Error(`toml-lines: no ${header} table found`);
  }
  const body = [];
  for (let i = start + 1; i < lines.length; i += 1) {
    if (/^\s*\[/.test(lines[i])) break;
    body.push(lines[i]);
  }
  return body;
}

export function tableEntries(text, table) {
  const body = tableLines(text, table);
  const entries = [];
  for (const line of body) {
    if (line.trim() === '' || line.trimStart().startsWith('#')) continue;
    const match = line.match(/^(\s*)([A-Za-z0-9_-]+)\s*=\s*(.*)$/);
    if (!match) {
      if (entries.length === 0) {
        throw new Error(`toml-lines: ${table} starts with a line this reader cannot place: ${line}`);
      }
      entries[entries.length - 1].lines.push(line);
      continue;
    }
    entries.push({ key: match[2], indent: match[1], value: match[3].trim(), lines: [line] });
  }
  return entries;
}

export function readStringArray(text, key, sourceName) {
  const lines = splitLines(text);
  const open = new RegExp(`^\\s*${key}\\s*=\\s*\\[`);
  let start = -1;
  for (let i = 0; i < lines.length; i += 1) {
    if (open.test(lines[i])) {
      start = i + 1;
      break;
    }
  }
  if (start === -1) {
    throw new Error(`${sourceName}: no "${key} = [" array found`);
  }
  const entries = [];
  for (let i = start; i < lines.length; i += 1) {
    const line = lines[i];
    if (/^\s*\]/.test(line)) {
      return entries;
    }
    const trimmed = contentOf(line).trim();
    if (trimmed === '' || trimmed.startsWith('#')) {
      continue;
    }
    const match = trimmed.match(/^"([^"]+)"\s*,?$/);
    if (!match) {
      throw new Error(`${sourceName}:${i + 1}: expected a quoted path, found: ${trimmed}`);
    }
    entries.push(match[1]);
  }
  throw new Error(`${sourceName}: "${key}" array is not closed`);
}
