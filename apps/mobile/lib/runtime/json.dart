import 'package:uuid/uuid.dart';

Map<String, dynamic> object(Object? value) =>
    value is Map ? Map<String, dynamic>.from(value) : <String, dynamic>{};

List<Map<String, dynamic>> objects(Object? value) =>
    value is List ? value.map(object).toList() : <Map<String, dynamic>>[];

String text(Object? value, [String fallback = '']) =>
    value is String ? value : fallback;

num number(Object? value, [num fallback = 0]) =>
    value is num ? value : fallback;

String? nodeIdHex(Object? value) {
  if (value is! List ||
      value.length != 32 ||
      value.any((byte) => byte is! int || byte < 0 || byte > 255)) {
    return null;
  }
  return value
      .cast<int>()
      .map((byte) => byte.toRadixString(16).padLeft(2, '0'))
      .join();
}

String newId() => const Uuid().v4();
