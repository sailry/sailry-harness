import 'package:flutter/foundation.dart';

import '../../../runtime/json.dart';
import '../../../runtime/session.dart';
import 'branch_card.dart';

/// Read-only Git status and upstream sync for the resources overview.
/// Failures leave the summary empty; the Git page reports them.
class GitSummary extends ChangeNotifier {
  HostConnection? _host;
  String? _worktree;
  int _generation = 0;
  Map<String, dynamic>? status;
  BranchSync? sync;

  bool get repository => status != null && status!['kind'] != 'directory';

  /// Switches to [worktree] without notifying, so it is safe during build.
  void track(HostConnection? host, String? worktree) {
    if (host?.id == _host?.id && worktree == _worktree) return;
    _host = host;
    _worktree = worktree;
    _generation++;
    status = null;
    sync = null;
    if (host != null && worktree != null) load();
  }

  Future<void> load() async {
    final host = _host;
    final worktree = _worktree;
    if (host == null || worktree == null || !host.connected) return;
    final generation = ++_generation;
    try {
      final data = {'worktree': worktree};
      final result = await host.command('inspect_git', data);
      if (generation != _generation) return;
      final next = object(result['data']);
      BranchSync? branches;
      if (next['kind'] != 'directory') {
        final listed = await host.command('list_git_branches', data);
        if (generation != _generation) return;
        final value = object(object(listed['data'])['sync']);
        branches = (
          upstream: value['upstream'] as String?,
          ahead: number(value['ahead']).toInt(),
          behind: number(value['behind']).toInt(),
        );
      }
      status = next;
      sync = branches;
    } catch (_) {
      if (generation != _generation) return;
      status = null;
      sync = null;
    }
    notifyListeners();
  }

  @override
  void dispose() {
    _generation++;
    super.dispose();
  }
}
