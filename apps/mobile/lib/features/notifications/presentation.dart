import 'dart:convert';

import '../../l10n/strings.dart';
import '../../runtime/json.dart';

String noticeTitle(Map<String, dynamic> notice, {Translator translate = tr}) =>
    object(notice['details'])['type'] == 'personal_kv_conflict'
    ? translate('personalKvConflict')
    : text(notice['title']).isNotEmpty
    ? text(notice['title'])
    : translate('conversationNew');

String? noticeRoute(Map<String, dynamic> id) {
  final node = nodeIdHex(id['node']);
  final entry = id['entry'] == null ? node : nodeIdHex(id['entry']);
  if (node == null || entry == null) return null;
  return node == entry ? node : '$entry/$node';
}

String? noticeSession(Map<String, dynamic> notice) {
  final session = notice['session'];
  if (session is String && session.isNotEmpty) return session;
  final target = object(object(notice['id'])['target'])['Session'];
  return target is String && target.isNotEmpty ? target : null;
}

bool completedSessionNotice(Map<String, dynamic> notice) =>
    notice['kind'] == 'Completed' &&
    notice['read'] != true &&
    noticeSession(notice) != null;

/// An inbox read first establishes a baseline per entry and target route.
/// Only subsequent confirmed reads on that route can produce new alerts.
class NoticeFreshness {
  final _seen = <String, Set<String>>{};

  Set<String> observe(
    List<Map<String, dynamic>> notices,
    Set<String> refreshedRoutes,
    Set<String> registeredRoutes,
  ) {
    _seen.removeWhere((route, _) => !registeredRoutes.contains(route));
    final current = {for (final route in refreshedRoutes) route: <String>{}};
    for (final notice in notices) {
      final id = object(notice['id']);
      current[noticeRoute(id)]?.add(jsonEncode(id));
    }
    final fresh = <String>{};
    for (final MapEntry(key: route, value: ids) in current.entries) {
      if (_seen[route] case final previous?) {
        fresh.addAll(ids.difference(previous));
      }
      _seen[route] = ids;
    }
    return fresh;
  }

  void clear() => _seen.clear();
}
