import '../../../content/image_reference.dart';
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
      _segments = object(view['segments']) {
    page = object(snapshot['page']);
    runs = objects(page['runs']);
    // Client selects visible response owners in shared execution order.
    final byTurn = {for (final run in runs) run['turn']: run};
    timeline = [
      for (final turn in (view['timeline'] as List? ?? const []))
        if (byTurn[turn] case final Map<String, dynamic> run) run,
    ];
    childRecords = objects(page['children']);
    latest = timeline.where((run) => run['kind'] != 'compaction').lastOrNull;
    for (final run in runs) {
      _runs.putIfAbsent(run['turn'], () => run);
      for (final notice in objects(run['notices'])) {
        if (notice['id'] case final String id) {
          _background.putIfAbsent(id, () => notice);
        }
      }
    }
    for (final task in objects(page['background'])) {
      if (task['id'] case final String id) _background[id] = task;
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
    for (final item in objects(view['images'])) {
      _images[(item['call'], item['index'])] = object(item['image']);
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

  /// Runs in transcript order.
  late List<Map<String, dynamic>> timeline;
  final List<Map<String, dynamic>> calls;

  /// Child session references keyed by session, as delivered by Client.
  final Map<String, dynamic> children;
  late final List<Map<String, dynamic>> childRecords;

  /// The newest run that is not a compaction.
  late final Map<String, dynamic>? latest;

  final Map<String, dynamic> _segments;
  final _runs = <Object?, Map<String, dynamic>>{};
  final _entries = <Object?, Map<String, dynamic>>{};
  final _drafts = <Object?, Map<String, dynamic>>{};
  final _turnEntries = <Object?, List<Map<String, dynamic>>>{};
  final _turnDrafts = <Object?, List<Map<String, dynamic>>>{};
  final _sources = <(Object?, Object?), Map<String, dynamic>>{};
  final _turnCalls = <Object?, List<Map<String, dynamic>>>{};
  final _activeChildTurns = <Object?>{};
  final _images = <(Object?, Object?), Map<String, dynamic>>{};
  final _background = <String, Map<String, dynamic>>{};

  /// The conversation image Client resolved for a reply's reference.
  Map<String, dynamic>? image(ImageReference reference) =>
      _images[(reference.call, reference.index)];

  /// The call whose source is the given entry part.
  Map<String, dynamic>? call(Object? entry, Object? index) =>
      _sources[(entry, index)];

  /// The task ID on the Client-paired result names the command this row started.
  /// Historical notices supply evidence only if the activity record is absent.
  Map<String, dynamic>? backgroundOf(Map<String, dynamic> call) {
    final response = object(call['response']);
    final index = response['index'];
    final parts = _entries[response['entry']]?['parts'];
    if (parts is! List || index is! int || index < 0 || index >= parts.length) {
      return null;
    }
    final part = object(parts[index]);
    if (part['kind'] != 'tool_result') return null;
    return _background[object(object(part['data'])['summary'])['task']];
  }

  List<Map<String, dynamic>> callsOf(Object? turn) => _turnCalls[turn] ?? [];

  bool canEdit(Object? turn) => editableTurn(_runs[turn]);

  /// Committed entries followed by drafts of a turn.
  List<Map<String, dynamic>> entriesOf(Object? turn) => [
    ...?_turnEntries[turn],
    ...?_turnDrafts[turn],
  ];

  /// A response owner's inputs and rows, split where delivered input joined it.
  List<SegmentContent> segmentsOf(Object? turn) =>
      segmentContents(_segments[turn], entries: _entries, drafts: _drafts);

  /// Whether a referenced child started by this turn is still running.
  bool hasActiveChild(Object? turn) => _activeChildTurns.contains(turn);

  /// Foreground children holding the turn's next model request, as Client
  /// reports on its latest segment.
  int? waitingOf(Object? turn) =>
      objects(_segments[turn]).lastOrNull?['waiting'] as int?;

  String worktree(Object? turn, Map<String, dynamic> session) =>
      text(_runs[turn]?['worktree'], text(session['worktree']));
}
