import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_bridge/api/connection.dart';

class RecoveryConnection extends Fake implements Connection {
  final requests = <String>[];
  String? code;
  Map<String, dynamic> result = {};

  @override
  Future<String> execute({required String request}) async {
    requests.add(request);
    return jsonEncode(
      code == null
          ? {'Ok': result}
          : {
              'Err': {'code': code, 'message': 'Fixture failure'},
            },
    );
  }
}

Map<String, dynamic> provider(String id, {bool enabled = true}) => {
  'id': id,
  'enabled': enabled,
  'credential': {'id': 'credential-$id'},
  'default_model': 'missing-model',
  'models': [
    {'id': '$id-first', 'default_effort': 'default'},
    {'id': '$id-second', 'default_effort': 'high'},
  ],
};

Map<String, dynamic> snapshot(
  String id, {
  Map<String, dynamic>? config,
  List<Map<String, dynamic>>? providers,
}) => {
  'projects': <Map<String, dynamic>>[
    {'id': 'project-$id'},
  ],
  'worktrees': <Map<String, dynamic>>[
    {'id': 'tree-$id', 'project': 'project-$id'},
  ],
  'providers': providers ?? [provider(id)],
  'defaults': {'revision': 4, 'config': config},
};

Map<String, dynamic> admitted(Map<String, dynamic> data) => {
  'id': 'created',
  'project': data['project'],
  'worktree': data['worktree'],
  'revision': 1,
  'config': data['config'],
};
