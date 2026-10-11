import 'dart:convert';

import 'package:webview_flutter/webview_flutter.dart';

Future<Map<String, dynamic>> readObject(
  WebViewController controller,
  String expression,
) async {
  final result = await controller.runJavaScriptReturningResult(
    'JSON.stringify($expression)',
  );
  // Android preserves evaluateJavascript's encoded string; WebKit unwraps it.
  final decoded = jsonDecode(result as String);
  final value = decoded is String ? jsonDecode(decoded) : decoded;
  if (value is! Map<String, dynamic>) {
    throw const FormatException('Expected a JavaScript object');
  }
  return value;
}
