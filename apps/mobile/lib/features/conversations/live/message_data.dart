import '../../../runtime/json.dart';

/// Rewind returns a recovery copy; editing replaces the original branch.
Map<String, dynamic>? historyBackup(
  String command,
  Map<String, dynamic> output,
) {
  if (command != 'rewind_conversation') return null;
  final backup = object(object(output['data'])['backup']);
  return backup['id'] is String ? backup : null;
}

String turnOwner(Map<String, dynamic> view, String turn) =>
    text(object(view['owners'])[turn], turn);

/// Replacement edits a whole task, not guidance consumed within another task.
bool editableTurn(Map<String, dynamic>? run) =>
    run?['kind'] == 'task' && run?['joined'] == null && run?['notice'] != true;

typedef InputContent = ({
  String turn,
  String text,
  List<Map<String, dynamic>> attachments,
  List<Map<String, dynamic>> references,
  bool canonical,
});

/// A reply row: its entry or draft, part index and displayed part.
typedef SelectedRow = (Map<String, dynamic>, int, Map<String, dynamic>);

/// Inputs followed by the reply rows produced before the next delivered input.
typedef SegmentContent = ({List<InputContent> inputs, List<SelectedRow> rows});

List<InputContent> inputContents(Object? inputs) => [
  for (final input in objects(inputs))
    (
      turn: input['turn'] as String,
      text: text(object(input['message'])['text']),
      attachments: objects(input['attachments']),
      references: objects(object(input['message'])['references']),
      canonical: input['canonical'] == true,
    ),
];

/// Reads one turn's Client segments in transcript order.
List<SegmentContent> segmentContents(
  Object? segments, {
  required Map<Object?, Map<String, dynamic>> entries,
  required Map<Object?, Map<String, dynamic>> drafts,
}) => [
  for (final segment in objects(segments))
    (
      inputs: inputContents(segment['inputs']),
      rows: selectedRows(segment['rows'], entries: entries, drafts: drafts),
    ),
];

/// Resolves selected rows against entries and drafts keyed by id, adding
/// Markdown citation links at the Client's source offsets.
List<SelectedRow> selectedRows(
  Object? selected, {
  required Map<Object?, Map<String, dynamic>> entries,
  required Map<Object?, Map<String, dynamic>> drafts,
}) {
  final rows = <SelectedRow>[];
  for (final row in objects(selected)) {
    final source = object(row['source']);
    final entry = (row['draft'] == true ? drafts : entries)[source['entry']]!;
    final part = {...object(row['part']), 'work': row['work']};
    if (part['kind'] == 'text') {
      part['data'] = citedText(
        text(part['data']),
        objects(entry['citations']),
        row['text_offset'] as int,
        row['text_last'] == true,
      );
    }
    rows.add((entry, source['index'] as int, part));
  }
  return rows;
}

Map<String, dynamic> sentInput(Map<String, dynamic> page, Object? turn) {
  final parts = objects(page['entries'])
      .where((entry) => entry['turn'] == turn && entry['author'] == 'user')
      .expand((entry) => objects(entry['parts']))
      .toList();
  return {
    'text': parts
        .where((part) => part['kind'] == 'text')
        .map((part) => text(part['data']))
        .join('\n'),
    'attachments': parts
        .where((part) => part['kind'] == 'attachment')
        .map((part) => object(part['data'])['id'])
        .toList(),
    'references': parts
        .where((part) => part['kind'] == 'reference')
        .map((part) => part['data'])
        .toList(),
  };
}

/// Provider offsets count Unicode scalars across an entry's text parts.
String citedText(
  String value,
  List<Map<String, dynamic>> citations,
  int offset,
  bool last,
) {
  final runes = value.runes.toList();
  final insertions = <int, List<String>>{};
  for (final (index, citation) in citations.indexed) {
    final end = citation['end'];
    final position =
        end is int &&
            (end > offset || end == 0 && offset == 0) &&
            end <= offset + runes.length
        ? end - offset
        : end == null && last
        ? runes.length
        : null;
    final uri = Uri.tryParse(text(citation['uri']));
    if (position == null ||
        uri == null ||
        !['http', 'https'].contains(uri.scheme) ||
        uri.host.isEmpty) {
      continue;
    }
    final href = uri
        .toString()
        .replaceAll('<', '%3C')
        .replaceAll('>', '%3E')
        .replaceAll('\\', '%5C');
    (insertions[position] ??= []).add(' [${index + 1}](<$href>)');
  }
  final result = StringBuffer();
  for (var i = 0; i <= runes.length; i++) {
    for (final link in insertions[i] ?? <String>[]) {
      result.write(link);
    }
    if (i < runes.length) result.writeCharCode(runes[i]);
  }
  return result.toString();
}
