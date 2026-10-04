import '../../../runtime/json.dart';

String turnWorktree(
  Map<String, dynamic> page,
  Map<String, dynamic> session,
  Object? turn,
) => text(
  objects(
    page['runs'],
  ).where((run) => run['turn'] == turn).firstOrNull?['worktree'],
  text(session['worktree']),
);

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
