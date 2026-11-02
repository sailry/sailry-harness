import 'package:flutter/foundation.dart';

import '../../../runtime/json.dart';
import '../../../runtime/session.dart';

/// Read-only branch labels for the registered worktrees in the draft picker.
class DraftBranches extends ChangeNotifier {
  DraftBranches(this.host);

  final HostConnection host;
  final Map<String, Map<String, dynamic>> _entries = {};
  String? _project;
  int _generation = 0;
  Object? error;

  Future<void> load(Map<String, dynamic>? worktree) async {
    final project = worktree?['project'] as String?;
    _project = project;
    final generation = ++_generation;
    error = null;
    _entries.clear();
    notifyListeners();
    if (worktree == null || project == null || !host.connected) return;
    try {
      final result = await host.command('list_worktrees', {
        'worktree': worktree['id'],
      });
      if (generation != _generation) return;
      for (final entry in objects(object(result['data'])['entries'])) {
        _entries[text(entry['path'])] = entry;
      }
    } catch (failure) {
      if (generation != _generation) return;
      error = failure;
    }
    notifyListeners();
  }

  String label(Map<String, dynamic>? worktree, String fallback) {
    if (worktree == null || worktree['project'] != _project) return fallback;
    final entry = _entries[text(worktree['path'])];
    final branch = text(entry?['branch']);
    if (branch.isNotEmpty) return branch;
    final head = text(entry?['head']);
    return head.isEmpty
        ? fallback
        : head.length > 7
        ? head.substring(0, 7)
        : head;
  }

  @override
  void dispose() {
    _generation++;
    super.dispose();
  }
}
