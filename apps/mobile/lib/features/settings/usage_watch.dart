import 'dart:convert';
import '../../runtime/session.dart';

/// Unifies the lifetime of the two existing Rust subscription handles only.
class UsageWatch {
  UsageWatch({required this.next, required this.close});
  final Future<String> Function() next;
  final Future<void> Function() close;
  static Future<UsageWatch> open(
    AppSession session,
    HostConnection? host,
    Map<String, dynamic> query,
  ) async {
    if (host != null) {
      final updates = await host.connection.watchUsage(
        query: jsonEncode(query),
      );
      return UsageWatch(
        next: updates.next,
        close: () async {
          await updates.close();
          updates.dispose();
        },
      );
    }
    final controller = session.controller;
    if (controller == null) throw StateError('controller is not ready');
    final updates = await controller.watchUsage(
      addresses: session.hosts.map((host) => host.address).toList(),
      query: jsonEncode(query),
    );
    return UsageWatch(
      next: updates.next,
      close: () async {
        await updates.close();
        updates.dispose();
      },
    );
  }
}

typedef UsageWatchFactory =
    Future<UsageWatch> Function(
      AppSession session,
      HostConnection? host,
      Map<String, dynamic> query,
    );
