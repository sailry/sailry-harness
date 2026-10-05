import 'package:flutter/material.dart';

import '../../../content/diff.dart';
import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../runtime/session.dart';
import '../../../ui/kit.dart';
import '../../resources/file_navigation.dart';
import '../../resources/file_page.dart';
import 'presentation.dart';

/// UI-owned inspection state. Diffs and recovery decisions remain on the Node.
class ChangeReview extends ChangeNotifier {
  ChangeReview(this.host, this.session, this.turn, {this.translate = tr});
  Translator translate;
  final HostConnection host;
  final String session, turn;
  Map<String, dynamic>? diff;
  List<Map<String, dynamic>>? _checkpoints;
  final done = <String>{};
  bool busy = false;
  bool _disposed = false;
  String? error;
  String? pending;
  String? _pendingCheckpoint;
  String? target;
  void _changed() {
    if (!_disposed) notifyListeners();
  }

  @override
  void dispose() {
    _disposed = true;
    super.dispose();
  }

  Future<void> load() async {
    if (busy || diff != null) return;
    busy = true;
    error = null;
    _changed();
    try {
      final output = await host.command('read_turn_diff', {
        'session': session,
        'turn': turn,
      });
      final data = object(output['data']);
      if (data['session'] != session || data['turn'] != turn) {
        throw StateError('Mismatched turn diff');
      }
      diff = data;
    } catch (failure) {
      error = failureLabel(failure, translate: translate);
    } finally {
      busy = false;
      _changed();
    }
  }

  bool restored(String? path) {
    final matching =
        _checkpoints
            ?.where((file) => path == null || file['path'] == path)
            .toList() ??
        [];
    return matching.isNotEmpty &&
        matching.every((file) => done.contains(file['id']));
  }

  Future<void> restore(String? path) async {
    if (busy) return;
    if (pending == null) target = path;
    busy = true;
    error = null;
    _changed();
    try {
      if (_checkpoints == null) {
        final files = <Map<String, dynamic>>[];
        String? before;
        do {
          final output = await host.command('list_file_checkpoints', {
            'session': session,
            'turn': turn,
            'before': before,
            'limit': 100,
          });
          final data = object(output['data']);
          if (data['session'] != session || data['turn'] != turn) {
            throw StateError('Mismatched checkpoint page');
          }
          files.addAll(
            objects(data['files']).where((file) {
              final outcome = object(file['outcome']);
              return outcome['kind'] == 'completed' &&
                  object(object(outcome['data'])['Ok'])['kind'] ==
                      'file_written';
            }),
          );
          before = data['next'] as String?;
        } while (before != null && !_disposed);
        _checkpoints = files;
      }
      // Node pages are newest first: repeated writes must unwind in that order.
      for (final file in _checkpoints!) {
        if (_disposed) break;
        final id = file['id'] as String;
        if (done.contains(id) || target != null && file['path'] != target) {
          continue;
        }
        if (pending != null && _pendingCheckpoint != id) {
          throw StateError('Mismatched recovery target');
        }
        Map<String, dynamic> output;
        try {
          output = pending == null
              ? await host.command('restore_file_checkpoint', {
                  'session': session,
                  'checkpoint': id,
                  'worktree': file['worktree'],
                })
              : await host.execute(pending!);
        } on CommandFailure catch (failure) {
          pending = failure.code == 'outcome_unknown' ? failure.request : null;
          _pendingCheckpoint = pending == null ? null : id;
          rethrow;
        }
        final data = object(output['data']);
        if (output['kind'] != 'file_restored' ||
            data['session'] != session ||
            data['checkpoint'] != id) {
          throw StateError('Mismatched checkpoint receipt');
        }
        done.add(id);
        pending = null;
        _pendingCheckpoint = null;
      }
    } catch (failure) {
      error = failureLabel(failure, translate: translate);
    } finally {
      busy = false;
      _changed();
    }
  }
}

class TurnChanges extends StatefulWidget {
  const TurnChanges({
    super.key,
    required this.host,
    required this.session,
    required this.turn,
    required this.worktree,
    this.canRestore,
  });
  final HostConnection host;
  final String session, turn, worktree;
  final bool Function()? canRestore;
  @override
  State<TurnChanges> createState() => _TurnChangesState();
}

class _TurnChangesState extends State<TurnChanges> {
  late final review = ChangeReview(widget.host, widget.session, widget.turn);
  bool _open = false;
  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    review.translate = context.tr;
  }

  @override
  void dispose() {
    review.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => ListenableBuilder(
    listenable: review,
    builder: (context, _) {
      final files = objects(review.diff?['files']);
      final added = files.fold<int>(
        0,
        (total, file) => total + number(file['additions']).toInt(),
      );
      final removed = files.fold<int>(
        0,
        (total, file) => total + number(file['deletions']).toInt(),
      );
      return OutlinedButton.icon(
        icon: const AppIcon('file', size: 16),
        label: Text(
          review.diff == null
              ? context.tr('turnChanges')
              : '${context.tr('turnChangesCount').replaceAll('{count}', '${files.length}')}  +$added −$removed',
        ),
        onPressed: _open
            ? null
            : () async {
                _open = true;
                try {
                  await pushPage(
                    context,
                    _ChangesPage(
                      review: review,
                      worktree: widget.worktree,
                      canRestore: () => widget.canRestore?.call() ?? false,
                    ),
                  );
                } finally {
                  _open = false;
                }
              },
      );
    },
  );
}

class _ChangesPage extends StatefulWidget {
  const _ChangesPage({
    required this.review,
    required this.worktree,
    required this.canRestore,
  });
  final ChangeReview review;
  final String worktree;
  final bool Function() canRestore;
  @override
  State<_ChangesPage> createState() => _ChangesPageState();
}

class _ChangesPageState extends State<_ChangesPage> {
  final documents = <String, FileDocument>{};
  @override
  void initState() {
    super.initState();
    Future.microtask(() {
      if (mounted) widget.review.load();
    });
  }

  Future<void> _restore(String? path) async {
    final review = widget.review;
    if (review.busy || !review.host.connected) return;
    if (review.pending == null) {
      if (!widget.canRestore()) return;
      final dirty = documents.entries.any(
        (entry) =>
            (path == null || entry.key.endsWith('/$path')) &&
            (entry.value.draft?.dirty == true ||
                entry.value.draft?.pending != null),
      );
      if (dirty) {
        review.error = context.tr('turnUndoUnsaved');
        review._changed();
        return;
      }
      final confirmed = await showAppDialog<bool>(
        context: context,
        builder: (context) => AlertDialog(
          title: Text(context.tr('turnUndo')),
          content: Text([?path, context.tr('turnUndoConfirm')].join('\n')),
          actions: [
            TextButton(
              onPressed: () => Navigator.pop(context, false),
              child: Text(context.tr('cancel')),
            ),
            FilledButton(
              onPressed: () => Navigator.pop(context, true),
              child: Text(context.tr('confirm')),
            ),
          ],
        ),
      );
      if (!mounted || confirmed != true || !widget.canRestore()) return;
    }
    final done = review.done.length;
    await review.restore(path);
    if (review.done.length != done) {
      documents.removeWhere(
        (_, document) =>
            document.draft?.dirty != true && document.draft?.pending == null,
      );
    }
  }

  @override
  Widget build(BuildContext context) => ListenableBuilder(
    listenable: widget.review,
    builder: (context, _) {
      final review = widget.review;
      final files = objects(review.diff?['files']);
      return PopScope(
        canPop: !review.busy,
        child: PageFrame(
          title: context.tr('turnChanges'),
          loading: review.busy,
          backEnabled: !review.busy,
          failure: review.diff == null && review.error != null
              ? FailureState(message: review.error!, onRetry: review.load)
              : null,
          actions: [
            TextButton(
              onPressed:
                  review.busy ||
                      !review.host.connected ||
                      review.diff == null ||
                      files.isEmpty ||
                      (review.pending == null && !widget.canRestore()) ||
                      review.restored(null)
                  ? null
                  : () => _restore(null),
              child: Text(
                context.tr(
                  review.pending != null ? 'messageCheck' : 'turnUndoAll',
                ),
              ),
            ),
          ],
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              if (review.error != null)
                Text(
                  review.error!,
                  style: TextStyle(color: Theme.of(context).colorScheme.error),
                ),
              if (review.done.isNotEmpty && review.error != null)
                Text(context.tr('turnUndoPartial')),
              if (review.diff?['partial'] == true)
                Text(context.tr('resourcePartial')),
              for (final file in files) ...[
                Row(
                  children: [
                    Expanded(
                      child: TextButton(
                        onPressed: review.busy || !review.host.connected
                            ? null
                            : () => openResourceFile(
                                context,
                                review.host,
                                widget.worktree,
                                text(file['path']),
                                documents: documents,
                              ),
                        child: Text(text(file['path'])),
                      ),
                    ),
                    TextButton(
                      onPressed:
                          review.busy ||
                              review.pending != null ||
                              !widget.canRestore() ||
                              review.restored(text(file['path'])) ||
                              !review.host.connected
                          ? null
                          : () => _restore(text(file['path'])),
                      child: Text(
                        context.tr(
                          review.restored(text(file['path']))
                              ? 'turnUndoDone'
                              : 'turnUndo',
                        ),
                      ),
                    ),
                  ],
                ),
                DiffView(diff: file),
                const SizedBox(height: 12),
              ],
              if (review.diff != null && files.isEmpty)
                Text(context.tr('noChanges')),
            ],
          ),
        ),
      );
    },
  );
}
