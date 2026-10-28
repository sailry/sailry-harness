import 'dart:convert';

import '../../../runtime/json.dart';

/// Mounted surfaces declare references; values and retry state remain in Client.
class ToolSelection {
  final _views = <Object, Set<(String, int)>>{};
  String? _last;

  String? update(
    Object owner,
    List<Map<String, dynamic>> calls, {
    bool retry = false,
  }) {
    final selected = <(String, int)>{};
    for (final call in calls) {
      final source = object(call['source']);
      if (source['entry'] case final String entry) {
        if (source['index'] case final int index) selected.add((entry, index));
      }
    }
    if (selected.isEmpty) {
      _views.remove(owner);
    } else {
      _views[owner] = selected;
    }
    final items = _views.values.expand((value) => value).toSet().toList()
      ..sort((a, b) {
        final order = a.$1.compareTo(b.$1);
        return order == 0 ? a.$2.compareTo(b.$2) : order;
      });
    Map<String, dynamic> item((String, int) value) => {
      'entry': value.$1,
      'index': value.$2,
    };
    final identity = jsonEncode(items.map(item).toList());
    if (_last == identity && !retry) return null;
    _last = identity;
    return jsonEncode({
      'items': items.map(item).toList(),
      'retry': retry ? selected.map(item).toList() : [],
    });
  }

  void reset() => _last = null;
}

Map<String, dynamic>? toolDetail(Map<String, dynamic> call) =>
    call['detail'] is Map ? object(call['detail']) : null;

bool detailFailed(Map<String, dynamic> call) {
  final state = object(call['detail_state']);
  return state['kind'] == 'failed' && state['retrying'] != true;
}

bool detailAllowsApproval(Map<String, dynamic> call) =>
    ['ready', 'too_large'].contains(object(call['detail_state'])['kind']);

bool detailAllowsDecision(Map<String, dynamic> call, String decision) =>
    decision == 'deny' || (decision == 'approve' && detailAllowsApproval(call));

/// Omitted ready values retain the previously delivered allocation for that revision.
Map<String, dynamic> mergeToolDetails(
  Map<String, dynamic> previous,
  Map<String, dynamic> next,
) {
  if (object(object(previous['snapshot'])['page'])['session'] !=
      object(object(next['snapshot'])['page'])['session']) {
    return next;
  }
  final existing = <(String, int), Map<String, dynamic>>{};
  for (final call in objects(previous['calls'])) {
    final source = object(call['source']);
    if (source['entry'] case final String entry) {
      if (source['index'] case final int index) existing[(entry, index)] = call;
    }
  }
  for (final value in next['calls'] as List? ?? const []) {
    if (value is! Map<String, dynamic>) continue;
    final call = value;
    if (call.containsKey('detail') ||
        object(call['detail_state'])['kind'] != 'ready') {
      continue;
    }
    final source = object(call['source']);
    if (source['entry'] is! String || source['index'] is! int) continue;
    final old = existing[(source['entry'] as String, source['index'] as int)];
    if (old == null) continue;
    final part = object(call['response'] ?? call['source']);
    final oldPart = object(old['response'] ?? old['source']);
    if (object(old['detail_state'])['kind'] == 'ready' &&
        old['turn'] == call['turn'] &&
        old['revision'] == call['revision'] &&
        old['id'] == call['id'] &&
        oldPart['entry'] == part['entry'] &&
        oldPart['index'] == part['index']) {
      call['detail'] = old['detail'];
    }
  }
  return next;
}
