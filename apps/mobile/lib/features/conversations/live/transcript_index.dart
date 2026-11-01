import '../../../runtime/json.dart';
import 'message_data.dart';
import 'turn_frame.dart';

/// Lookups over one Client projection, built once per update so each lazily
/// built turn avoids rescanning every call, entry and child. This indexes the
/// Client's output for presentation; it never reassociates or reduces it.
class TranscriptIndex {
  TranscriptIndex(this.view)
    : snapshot = object(view['snapshot']),
      calls = objects(view['calls']),
      children = object(object(view['children'])['sessions']),
      _rows = object(view['rows']),
      _inputs = object(view['inputs']) {
    page = object(snapshot['page']);
    runs = objects(page['runs']);
    childRecords = objects(page['children']);
    latest = runs.where((run) => run['kind'] != 'compaction').lastOrNull;
    for (final run in runs) {
      _runs.putIfAbsent(run['turn'], () => run);
    }
    for (final entry in objects(page['entries'])) {
      _entries[entry['id']] = entry;
      (_turnEntries[entry['turn']] ??= []).add(entry);
    }
    for (final draft in objects(snapshot['drafts'])) {
      _drafts[draft['id']] = draft;
      (_turnDrafts[draft['turn']] ??= []).add(draft);
    }
    for (final call in calls) {
      final source = object(call['source']);
      _sources.putIfAbsent((source['entry'], source['index']), () => call);
      (_turnCalls[call['turn']] ??= []).add(call);
    }
    for (final reference in children.values) {
      final index = object(reference)['index'];
      if (index is! int || index >= childRecords.length) continue;
      final child = childRecords[index];
      if (activeRun(object(child['run']))) {
        _activeChildTurns.add(object(child['origin'])['turn']);
      }
    }
  }

  final Map<String, dynamic> view;
  final Map<String, dynamic> snapshot;
  late final Map<String, dynamic> page;
  late final List<Map<String, dynamic>> runs;
  final List<Map<String, dynamic>> calls;

  /// Child session references keyed by session, as delivered by Client.
  final Map<String, dynamic> children;
  late final List<Map<String, dynamic>> childRecords;

  /// The newest run that is not a compaction.
  late final Map<String, dynamic>? latest;

  final Map<String, dynamic> _rows;
  final Map<String, dynamic> _inputs;
  final _runs = <Object?, Map<String, dynamic>>{};
  final _entries = <Object?, Map<String, dynamic>>{};
  final _drafts = <Object?, Map<String, dynamic>>{};
  final _turnEntries = <Object?, List<Map<String, dynamic>>>{};
  final _turnDrafts = <Object?, List<Map<String, dynamic>>>{};
  final _sources = <(Object?, Object?), Map<String, dynamic>>{};
  final _turnCalls = <Object?, List<Map<String, dynamic>>>{};
  final _activeChildTurns = <Object?>{};

  /// The call whose source is the given entry part.
  Map<String, dynamic>? call(Object? entry, Object? index) =>
      _sources[(entry, index)];

  List<Map<String, dynamic>> callsOf(Object? turn) => _turnCalls[turn] ?? [];

  /// Committed entries followed by drafts of a turn.
  List<Map<String, dynamic>> entriesOf(Object? turn) => [
    ...?_turnEntries[turn],
    ...?_turnDrafts[turn],
  ];

  List<(Map<String, dynamic>, int, Map<String, dynamic>)> rowsOf(
    Object? turn,
  ) => selectedRows(_rows[turn], entries: _entries, drafts: _drafts);

  List<InputContent> inputsOf(Object? turn) => inputContents(_inputs[turn]);

  /// Whether a referenced child started by this turn is still running.
  bool hasActiveChild(Object? turn) => _activeChildTurns.contains(turn);

  String worktree(Object? turn, Map<String, dynamic> session) =>
      text(_runs[turn]?['worktree'], text(session['worktree']));
}
