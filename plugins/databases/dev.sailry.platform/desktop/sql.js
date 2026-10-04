// SQL interaction policy from builtin Databases at ef5e82ea (GPL-3.0-only).
// Core projects integer cells as decimal strings before crossing the UI boundary.
const hex = bytes => Array.from(bytes, byte => byte.toString(16).padStart(2,'0')).join('');
const utf8 = value => {
  const bytes = [];
  for (const character of value) {
    const point = character.codePointAt(0);
    if (point < 0x80) bytes.push(point);
    else if (point < 0x800) bytes.push(0xc0 | point >> 6,0x80 | point & 63);
    else if (point < 0x10000) bytes.push(0xe0 | point >> 12,0x80 | point >> 6 & 63,0x80 | point & 63);
    else bytes.push(0xf0 | point >> 18,0x80 | point >> 12 & 63,0x80 | point >> 6 & 63,0x80 | point & 63);
  }
  return bytes;
};
export function identifier(engine,value) {
  const quote = engine === 'mysql' ? '`' : '"';
  return quote + value.split(quote).join(quote + quote) + quote;
}
export const tableName = (engine,table) => `${identifier(engine,table.schema)}.${identifier(engine,table.name)}`;
export function literal(engine,cell) {
  const {kind,value} = cell;
  switch (kind) {
    case 'null': return 'NULL';
    case 'integer': {
      const text = typeof value === 'string' ? value : Number.isSafeInteger(value) ? String(value) : '';
      return /^-?\d+$/.test(text) ? text : null;
    }
    case 'real': return typeof value === 'string' && value.length > 0 && /^[+\-.eE\d]+$/.test(value) && Number.isFinite(Number(value)) ? value : null;
    case 'text':
      if (engine === 'mysql' && /[\\\0]/.test(value)) return `CONVERT(X'${hex(utf8(value))}' USING utf8mb4)`;
      if (engine === 'postgres') return value.includes('\0') ? null : `E'${value.replace(/\\/g,'\\\\').replace(/'/g,"''")}'`;
      if (engine === 'sqlite' && value.includes('\0')) return `CAST(X'${hex(utf8(value))}' AS TEXT)`;
      return `'${value.replace(/'/g,"''")}'`;
    case 'blob': {
      if (!Array.isArray(value) || value.some(byte=>!Number.isInteger(byte) || byte<0 || byte>255)) return null;
      return engine === 'postgres' ? `decode('${hex(value)}', 'hex')` : `X'${hex(value)}'`;
    }
    default: return null;
  }
}
export function primaryKeys(engine,table) {
  const name = literal(engine,{kind:'text',value:table.name}), schema = literal(engine,{kind:'text',value:table.schema});
  if (name === null || schema === null) return null;
  if (engine === 'sqlite') return `SELECT name FROM pragma_table_info(${name}, ${schema}) WHERE pk > 0 ORDER BY pk`;
  if (engine === 'mysql') return `SELECT COLUMN_NAME FROM information_schema.KEY_COLUMN_USAGE WHERE TABLE_SCHEMA = ${schema} AND TABLE_NAME = ${name} AND CONSTRAINT_NAME = 'PRIMARY' ORDER BY ORDINAL_POSITION`;
  return `SELECT a.attname FROM pg_catalog.pg_index i JOIN pg_catalog.pg_class c ON c.oid = i.indrelid JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace JOIN LATERAL unnest(i.indkey) WITH ORDINALITY AS k(attnum, ord) ON k.ord <= i.indnkeyatts JOIN pg_catalog.pg_attribute a ON a.attrelid = c.oid AND a.attnum = k.attnum WHERE i.indisprimary AND n.nspname = ${schema} AND c.relname = ${name} ORDER BY k.ord`;
}
export function statement(engine,table,keys,columns,row,kind) {
  if (!columns.length || columns.length !== row.length || !['insert','update','delete'].includes(kind)) return null;
  const target = tableName(engine,table), values = row.map(value=>literal(engine,value)), names = columns.map(column=>identifier(engine,column));
  if (values.includes(null)) return null;
  if (kind === 'insert') return `INSERT INTO ${target} (${names.join(', ')}) VALUES (${values.join(', ')});`;
  if (!keys.length) return null;
  const predicates = [];
  for (const key of keys) {
    const index = columns.indexOf(key);
    if (index < 0 || row[index].kind === 'null') return null;
    predicates.push(`${names[index]} = ${values[index]}`);
  }
  const where = predicates.join(' AND ');
  if (kind === 'delete') return `DELETE FROM ${target} WHERE ${where};`;
  const assignments = columns.flatMap((column,index)=>keys.includes(column) ? [] : [`${names[index]} = ${values[index]}`]);
  return assignments.length ? `UPDATE ${target} SET ${assignments.join(', ')} WHERE ${where};` : null;
}
export function statements(engine,table,keys,columns,rows,kind) {
  if (!rows.length) return null;
  const sql = rows.map(row=>statement(engine,table,keys,columns,row,kind));
  if (sql.includes(null)) return null;
  if (kind !== 'delete' || sql.length === 1) return sql.join('\n');
  const prefix = `DELETE FROM ${tableName(engine,table)} WHERE `;
  return prefix + sql.map(value=>`(${value.slice(prefix.length,-1)})`).join(' OR ') + ';';
}
export function structure(engine,table) {
  const name = literal(engine,{kind:'text',value:table.name}), schema = literal(engine,{kind:'text',value:table.schema});
  if (name === null || schema === null) return null;
  if (engine === 'sqlite') return `SELECT name, type, "notnull", dflt_value, pk, hidden FROM pragma_table_xinfo(${name}, ${schema}) ORDER BY cid`;
  if (engine === 'mysql') return `SELECT COLUMN_NAME AS \`Field\`, COLUMN_TYPE AS \`Type\`, COLLATION_NAME AS \`Collation\`, IS_NULLABLE AS \`Null\`, COLUMN_KEY AS \`Key\`, COLUMN_DEFAULT AS \`Default\`, EXTRA AS \`Extra\`, COLUMN_COMMENT AS \`Comment\` FROM information_schema.COLUMNS WHERE TABLE_SCHEMA = ${schema} AND TABLE_NAME = ${name} ORDER BY ORDINAL_POSITION`;
  return `SELECT column_name, data_type, udt_name, is_nullable, column_default, is_identity, identity_generation, is_generated, generation_expression FROM information_schema.columns WHERE table_schema = ${schema} AND table_name = ${name} ORDER BY ordinal_position`;
}
export function definition(engine,table) {
  if (engine === 'postgres') return null;
  if (engine === 'mysql') return `SHOW CREATE TABLE ${tableName(engine,table)}`;
  const name = literal(engine,{kind:'text',value:table.name});
  return name === null ? null : `SELECT sql FROM ${identifier(engine,table.schema)}.sqlite_schema WHERE type IN ('table', 'view') AND name = ${name}`;
}
export function definitionText(engine,result) {
  if (result.truncated || result.rows.length !== 1) return null;
  const cell = result.rows[0][engine === 'mysql' ? 1 : 0];
  return cell?.kind === 'text' && cell.value.trim() ? cell.value : null;
}
