// Tool definitions and SQL presentation policy belong to the ordinary package.
export const definitions = {
  database_catalog:{
    description:"List databases, or tables in a selected database, using a saved connection. Credentials remain on the execution Node.",
    handler:"catalog",operation:"databases.catalog",presentation:"details",
  },
  database_query:{
    description:"Run one read-only SQL statement without approval. The execution connection is forced read-only. Use this for SELECT, inspecting schema, and analysis. Results also appear in the database workspace. Use database_execute for writes; never try to disable read-only protection.",
    handler:"query",operation:"databases.query",presentation:"content",
  },
  database_execute:{
    description:"Execute one SQL write statement under the session permission policy. Use for INSERT, UPDATE, DELETE, and schema changes. The saved connection's read-only setting still applies. Invoke this tool directly instead of asking for approval in chat. Do not retry a denied or uncertain write automatically.",
    handler:"execute",operation:"databases.execute",presentation:"details",
  },
};

export function label(profile) {
  const engine = {sqlite:'SQLite',mysql:'MySQL',postgres:'PostgreSQL'}[profile.engine];
  const quoting = profile.engine === 'mysql'
    ? 'backtick-quoted identifiers; do not use double quotes for table or database names'
    : profile.engine === 'postgres'
      ? 'double-quoted identifiers; qualify tables with schema, not database'
      : 'double-quoted identifiers';
  return `${profile.name}; engine=${engine}; default_database=${profile.database}; read_only=${profile.read_only}; ${quoting}`;
}

export function parameters(name,profiles) {
  const choices = profiles.map(profile=>[profile.id,label(profile)]);
  const properties = {connection:{type:'string',enum:choices.map(([id])=>id),description:JSON.stringify(choices)},
    database:{type:'string',description:'Database name; omit to use the saved default'}};
  const required = ['connection'];
  if (name !== 'database_catalog') { properties.sql={type:'string'};required.push('sql'); }
  return {type:'object',properties,required,additionalProperties:false};
}
