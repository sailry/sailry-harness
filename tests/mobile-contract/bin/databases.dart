import 'dart:convert';
import 'dart:io';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'package:sailry_bridge/api.dart';
import 'package:sailry_bridge/frb_generated.dart';
import 'conversation.dart' show check, execute;

Future<void> main() async {
  final env = Platform.environment;
  await RustLib.init(
    externalLibrary: ExternalLibrary.open(env['SAILRY_BRIDGE_LIBRARY']!),
  );
  final path = env['SAILRY_CONTROLLER_PROFILE']!;
  var controller = await Controller.open(
    path: path,
    internet: false,
    relays: [],
  );
  final address = await controller.pair(ticket: env['SAILRY_INVITATION']!);
  var connection = await controller.connect(address: address);
  for (final spec in jsonDecode(env['SAILRY_DATABASE_PROFILES']!)) {
    var profile = (await execute(connection, 'save_database', {
      'profile': spec,
      'expected_revision': 0,
      'password': spec['connection']['kind'] == 'sqlite'
          ? null
          : env['SAILRY_DATABASE_PASSWORD'],
    }))['data'];
    check(
      (await execute(connection, 'check_database', {
            'profile': profile['id'],
            'expected_revision': profile['revision'],
          }))['data']['kind'] ==
          'connected',
      'execution Node connects to the database',
    );
    Map<String, dynamic> query(String sql) => {
      'read_only': false,
      'profile': profile['id'],
      'expected_revision': profile['revision'],
      'sql': sql,
      'row_limit': 2,
      'timeout_ms': 5000,
    };
    await execute(
      connection,
      'query_database',
      query('CREATE TABLE items(id INTEGER, value TEXT)'),
    );
    final request = await connection.prepare(
      command: jsonEncode({
        'kind': 'query_database',
        'data': query(
          "INSERT INTO items VALUES (1, 'value 中文'), (2, NULL), (3, 'last')",
        ),
      }),
    );
    final inserted = await connection.execute(request: request);
    check(
      jsonDecode(inserted)['Ok']['data']['data']['affected_rows'] == 3,
      'writes report affected rows',
    );
    final rows = (await execute(
      connection,
      'query_database',
      query('SELECT * FROM items ORDER BY id'),
    ))['data']['data'];
    check(
      jsonEncode(rows['columns']) == '["id","value"]' &&
          rows['rows'].length == 2 &&
          rows['truncated'] == true,
      'column order and truncation survive FFI',
    );
    check(
      rows['rows'][0][1]['value'] == 'value 中文' &&
          rows['rows'][1][1]['kind'] == 'null',
      'Unicode and SQL null remain distinct',
    );
    final revision = profile['revision'];
    profile = (await execute(connection, 'save_database', {
      'profile': {...profile, 'read_only': true},
      'expected_revision': revision,
      'password': null,
    }))['data'];
    final denied = jsonDecode(
      await connection.execute(
        request: await connection.prepare(
          command: jsonEncode({
            'kind': 'query_database',
            'data': query('DELETE FROM items'),
          }),
        ),
      ),
    );
    check(denied['Err'] != null, 'read-only profile denies mutation');
    final updates = await connection.watch();
    final view = jsonDecode(
      await updates.next().timeout(const Duration(seconds: 10)),
    );
    check(
      view['snapshot']['databases'].single['read_only'] == true,
      'shared projection carries the saved profile',
    );
    check(
      !jsonEncode(view).contains(env['SAILRY_DATABASE_PASSWORD']!),
      'database password stays on execution Node',
    );
    await updates.close();
    updates.dispose();
    await controller.close();
    connection.dispose();
    controller.dispose();
    controller = await Controller.open(path: path, internet: false, relays: []);
    connection = await controller.connect(address: address);
    check(
      await connection.execute(request: request) == inserted,
      'reconnect observes the same write result without replay',
    );
    final count = (await execute(
      connection,
      'query_database',
      query('SELECT COUNT(*) AS count FROM items'),
    ))['data']['data'];
    check(
      count['rows'][0][0]['value'].toString() == '3',
      'write happened only once',
    );
    final stale = jsonDecode(
      await connection.execute(
        request: await connection.prepare(
          command: jsonEncode({
            'kind': 'remove_database',
            'data': {'profile': profile['id'], 'expected_revision': revision},
          }),
        ),
      ),
    );
    check(
      stale['Err']['code'] == 'revision_conflict',
      'stale removal cannot discard newer profile',
    );
    await execute(connection, 'remove_database', {
      'profile': profile['id'],
      'expected_revision': profile['revision'],
    });
    check(
      (await execute(connection, 'list_databases', null))['data'].isEmpty,
      'profile removal stays on execution Node',
    );
  }
  await controller.close();
  connection.dispose();
  controller.dispose();
  RustLib.dispose();
  stdout.writeln('Dart SQLite, PostgreSQL and MySQL workflows passed');
}
