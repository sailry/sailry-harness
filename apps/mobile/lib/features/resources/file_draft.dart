import '../../runtime/session.dart';
import '../../runtime/json.dart';

/// A UI draft retains the revision read from the execution Node until a save succeeds.
class FileDraft {
  FileDraft(Map<String, dynamic> content)
    : text = content['text'] as String? ?? '',
      saved = content['text'] as String? ?? '',
      revision = content['revision'] as String?,
      truncated = content['truncated'] == true;
  String text;
  String saved;
  String? revision;
  bool truncated;
  String? pending;
  String? _submitted;
  bool get dirty => text != saved;
  bool get editable => !truncated && revision != null;

  Future<void> save(HostConnection host, String worktree, String path) async {
    if (!editable) throw StateError('A partial file cannot be saved');
    final submitted = pending == null ? text : _submitted!;
    _submitted = submitted;
    Map<String, dynamic> output;
    try {
      output = pending == null
          ? await host.command('write_file', {
              'worktree': worktree,
              'path': path,
              'text': submitted,
              'expected_revision': revision,
            })
          : await host.execute(pending!);
    } on CommandFailure catch (error) {
      pending = error.code == 'outcome_unknown' ? error.request : null;
      rethrow;
    }
    final data = object(output['data']);
    revision = data['revision'] as String;
    saved = submitted;
    pending = null;
  }

  void reload(Map<String, dynamic> content) {
    if (pending != null) {
      throw StateError('Resolve the pending save before reloading');
    }
    text = content['text'] as String;
    saved = text;
    revision = content['revision'] as String?;
    truncated = content['truncated'] == true;
  }
}
