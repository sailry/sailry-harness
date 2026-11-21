import '../../../content/image_reference.dart';
import '../../../runtime/json.dart';
import 'message_data.dart';
import 'turn_frame.dart';

Map<String, dynamic> _record(Object? value) =>
    value is Map<String, dynamic> ? value : object(value);

List<Map<String, dynamic>> _records(Object? value) =>
    value is List ? value.cast<Map<String, dynamic>>() : const [];

/// Lookups over one Client projection, built once per update so each lazily
/// built turn avoids rescanning every call, entry and child. This indexes the
/// Client's output for presentation; it never reassociates or reduces it.
class TranscriptIndex {
  TranscriptIndex(this.view)
    : snapshot = _record(view['snapshot']),
      calls = _records(view['calls']),
      children = _record(_record(view['children'])['sessions']),
      _segments = _record(view['segments']) {
    page = _record(snapshot['page']);
    runs = _records(page['runs']);
    childRecords = _records(page['children']);
    for (final run in runs) {
      _runs.putIfAbsent(run['turn'], () => run);
      if (activeRun(run)) {
        latestActive = run;
        if (run['status'] != 'queued') active.add(run);
      }
      for (final notice in _records(run['notices'])) {
        if (notice['id'] case final String id) {
          _background.putIfAbsent(id, () => notice);
        }
      }
    }
    // Client selects visible response owners in shared execution order.
    for (final turn in (view['timeline'] as List? ?? const [])) {
      if (_runs[turn] case final run?) {
        _positions[turn] = timeline.length;
        timeline.add(run);
        if (run['kind'] != 'compaction') latest = run;
      }
    }
    for (final task in _records(page['background'])) {
      if (task['id'] case final String id) _background[id] = task;
    }
    for (final entry in _records(page['entries'])) {
      _entries[entry['id']] = entry;
      (_turnEntries[entry['turn']] ??= []).add(entry);
    }
    for (final draft in _records(snapshot['drafts'])) {
      _drafts[draft['id']] = draft;
      (_turnDrafts[draft['turn']] ??= []).add(draft);
    }
    for (final call in calls) {
      final source = _record(call['source']);
      _sources.putIfAbsent((source['entry'], source['index']), () => call);
      (_turnCalls[call['turn']] ??= []).add(call);
    }
    for (final item in _records(view['images'])) {
      _images[(item['call'], item['index'])] = _record(item['image']);
    }
    for (final reference in children.values) {
      final index = _record(reference)['index'];
      if (index is! int || index >= childRecords.length) continue;
      final child = childRecords[index];
      if (activeRun(_record(child['run']))) {
        _activeChildTurns.add(_record(child['origin'])['turn']);
      }
    }
  }

  final Map<String, dynamic> view;
  final Map<String, dynamic> snapshot;
  late final Map<String, dynamic> page;
  late final List<Map<String, dynamic>> runs;

  /// Runs in transcript order.
  final timeline = <Map<String, dynamic>>[];
  final active = <Map<String, dynamic>>[];
  Map<String, dynamic>? latestActive;
  final List<Map<String, dynamic>> calls;

  /// Child session references keyed by session, as delivered by Client.
  final Map<String, dynamic> children;
  late final List<Map<String, dynamic>> childRecords;

  /// The newest run that is not a compaction.
  Map<String, dynamic>? latest;

  final Map<String, dynamic> _segments;
  final _runs = <Object?, Map<String, dynamic>>{};
  final _positions = <Object?, int>{};
  final _entries = <Object?, Map<String, dynamic>>{};
  final _drafts = <Object?, Map<String, dynamic>>{};
  final _turnEntries = <Object?, List<Map<String, dynamic>>>{};
  final _turnDrafts = <Object?, List<Map<String, dynamic>>>{};
  final _sources = <(Object?, Object?), Map<String, dynamic>>{};
  final _turnCalls = <Object?, List<Map<String, dynamic>>>{};
  final _activeChildTurns = <Object?>{};
  final _images = <(Object?, Object?), Map<String, dynamic>>{};
  final _background = <String, Map<String, dynamic>>{};

  bool containsTurn(Object? turn) => _runs.containsKey(turn);

  Map<String, dynamic>? runOf(Object? turn) => _runs[turn];

  int? positionOf(Object? turn) => _positions[turn];

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
