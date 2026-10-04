import 'dart:async';
import 'dart:convert';
import 'package:sailry_bridge/api/commands.dart';

class CommandFixture implements CommandUpdates {
  final values = StreamController<String>();
  late final iterator = StreamIterator(values.stream);
  bool closed = false;
  void emit(Map<String, dynamic> view) => values.add(jsonEncode(view));
  @override
  Future<String> next() async {
    if (await iterator.moveNext()) return iterator.current;
    throw StateError('closed');
  }

  @override
  Future<void> close() async {
    if (closed) return;
    closed = true;
    unawaited(values.close());
  }

  @override
  void dispose() {}
  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}
