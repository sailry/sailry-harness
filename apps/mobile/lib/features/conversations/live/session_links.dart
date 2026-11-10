import 'package:sailry_bridge/api/rules.dart' as rules;

import '../../../runtime/json.dart';

/// Client owns the reply link format; Dart never parses it itself.
typedef SessionLinkParser = String? Function(String link);

String? _clientParse(String link) => rules.sessionLink(link: link);

/// The session a reply link names, with Client's child reference when this
/// conversation delegated it. Null for links that name no session.
({String id, Map<String, dynamic>? child})? linkedSession(
  String link,
  Map<String, dynamic> view, {
  SessionLinkParser parse = _clientParse,
}) {
  final id = parse(link);
  if (id == null) return null;
  final child = object(object(view['children'])['sessions'])[id];
  return (id: id, child: child is Map ? object(child) : null);
}
