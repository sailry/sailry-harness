import 'package:flutter/material.dart';
import '../../l10n/strings.dart';
import '../../runtime/json.dart';
import '../../runtime/notices.dart';
import '../../runtime/session.dart';
import '../../ui/kit.dart';
import '../../ui/toast.dart';
import 'host_file_picker.dart';

class ProjectForm extends StatefulWidget {
  const ProjectForm({super.key, required this.host, this.project})
    : removing = false;
  const ProjectForm.remove({
    super.key,
    required this.host,
    required Map<String, dynamic> this.project,
  }) : removing = true;

  final HostConnection host;

  /// The project being edited, as the host's snapshot lists it; null adds one.
  final Map<String, dynamic>? project;
  final bool removing;
  @override
  State<ProjectForm> createState() => ProjectFormState();
}

class ProjectFormState extends State<ProjectForm> {
  final _name = TextEditingController();
  final _path = TextEditingController();
  late final _kind = widget.removing
      ? 'remove_project'
      : widget.project == null
      ? 'register_project'
      : 'update_project';
  late final _write = _ProjectWrite.retain(
    widget.host,
    _kind,
    widget.project?['id'] as String?,
  );
  late final _changes = Listenable.merge([widget.host, _write]);

  @override
  void initState() {
    super.initState();
    final draft = _write.input ?? widget.project;
    _name.text = text(draft?['name']);
    _path.text = text(draft?['path']);
  }

  @override
  void dispose() {
    _name.dispose();
    _path.dispose();
    _write.release();
    super.dispose();
  }

  Future<void> _save() async {
    if (_write.busy || !widget.host.connected) return;
    final project = widget.project;
    final name = _name.text.trim(), path = _path.text.trim();
    if (_write.pending == null &&
        !widget.removing &&
        (name.isEmpty || path.isEmpty)) {
      return;
    }
    try {
      await _write.submit({
        'expected': ?project,
        if (!widget.removing) ...{'name': name, 'path': path},
        if (project == null) 'visibility': 'shared',
        if (project != null && !widget.removing)
          'appearance': project['appearance'] ?? <String, dynamic>{},
      });
      if (mounted && ModalRoute.of(context)?.isCurrent == true) {
        Navigator.pop(context);
      }
    } catch (failure) {
      if (!mounted) return;
      if (failure is CommandFailure &&
          failure.details['kind'] == 'project_removal' &&
          failure.details['terminals_may_be_closed'] == true) {
        showToast(
          context,
          '${failureText(failure, translate: context.tr)} '
          '${context.tr('projectRemovalMayHaveClosedTerminals')}',
        );
      } else {
        showFailure(context, failure);
      }
    }
  }

  Future<void> _browse() async {
    final path = await pushPage<String>(
      context,
      HostFilePicker(
        host: widget.host,
        directoryOnly: true,
        initialPath: _path.text.trim().isEmpty ? null : _path.text.trim(),
      ),
    );
    if (!mounted || path == null) return;
    setState(() {
      _path.text = path;
      if (_name.text.trim().isEmpty) {
        _name.text =
            path
                .replaceAll('\\', '/')
                .split('/')
                .where((part) => part.isNotEmpty)
                .lastOrNull ??
            path;
      }
    });
  }

  @override
  Widget build(BuildContext context) => ListenableBuilder(
    listenable: _changes,
    builder: (context, _) {
      final locked = _write.busy || _write.pending != null;
      final colors = Theme.of(context).colorScheme;
      return FormBody(
        children: [
          if (widget.removing) ...[
            Text(
              text(widget.project?['name']),
              style: Theme.of(context).textTheme.titleMedium,
            ),
            Text(context.tr('removeProjectWarning')),
          ] else ...[
            TextField(
              controller: _name,
              enabled: !locked,
              decoration: InputDecoration(
                labelText: context.tr('hostProjectName'),
              ),
            ),
            TextField(
              controller: _path,
              enabled: !locked,
              decoration: InputDecoration(
                labelText: context.tr('hostProjectPath'),
                suffixIcon: IconButton(
                  tooltip: context.tr('hostChooseDirectory'),
                  onPressed: locked ? null : _browse,
                  icon: const AppIcon('folder'),
                ),
              ),
            ),
          ],
          FilledButton(
            style: widget.removing && _write.pending == null
                ? FilledButton.styleFrom(
                    backgroundColor: colors.error,
                    foregroundColor: colors.onError,
                  )
                : null,
            onPressed: _write.busy || !widget.host.connected ? null : _save,
            child: Text(
              context.tr(
                _write.pending != null
                    ? 'conversationCheckResult'
                    : widget.removing
                    ? 'delete'
                    : 'save',
              ),
            ),
          ),
        ],
      );
    },
  );
}

/// A modal may close before a request settles. Its host keeps the original
/// request and submitted fields available when the operation is reopened.
class _ProjectWrite extends ChangeNotifier {
  _ProjectWrite(this.host, this.key);

  static final _writes = Expando<Map<(String, String?), _ProjectWrite>>();

  static _ProjectWrite retain(HostConnection host, String kind, String? id) {
    final writes = _writes[host] ??= {};
    final key = (kind, id);
    final write = writes.putIfAbsent(key, () => _ProjectWrite(host, key));
    write._views++;
    return write;
  }

  final HostConnection host;
  final (String, String?) key;
  Map<String, dynamic>? input;
  String? pending;
  bool busy = false;
  int _views = 0;

  void release() {
    _views--;
    _releaseIfSettled();
  }

  void _releaseIfSettled() {
    if (_views != 0 || busy || pending != null) return;
    _writes[host]?.remove(key);
    dispose();
  }

  Future<void> submit(Map<String, dynamic> data) async {
    busy = true;
    input ??= data;
    notifyListeners();
    try {
      final request = pending;
      if (request == null) {
        await host.command(key.$1, input!);
      } else {
        await host.check(request);
      }
      pending = null;
    } on CommandFailure catch (error) {
      if (error.retained) pending = error.request;
      if (error.rejected) pending = null;
      rethrow;
    } finally {
      if (pending == null) input = null;
      busy = false;
      notifyListeners();
      _releaseIfSettled();
    }
  }
}
