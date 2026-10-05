import 'package:flutter/material.dart';
import '../../l10n/strings.dart';
import '../../runtime/session.dart';
import '../../runtime/notices.dart';
import '../../ui/kit.dart';
import 'host_file_picker.dart';

class ProjectForm extends StatefulWidget {
  const ProjectForm({super.key, required this.host});
  final HostConnection host;
  @override
  State<ProjectForm> createState() => ProjectFormState();
}

class ProjectFormState extends State<ProjectForm> {
  final _name = TextEditingController();
  final _path = TextEditingController();
  bool _busy = false;
  @override
  void dispose() {
    _name.dispose();
    _path.dispose();
    super.dispose();
  }

  Future<void> _save() async {
    if (_busy || _name.text.trim().isEmpty || _path.text.trim().isEmpty) return;
    setState(() => _busy = true);
    try {
      await widget.host.command('register_project', {
        'name': _name.text.trim(),
        'path': _path.text.trim(),
      });
      if (mounted) Navigator.pop(context);
    } catch (failure) {
      if (mounted) showFailure(context, failure);
    } finally {
      if (mounted) setState(() => _busy = false);
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
  Widget build(BuildContext context) => FormBody(
    children: [
      TextField(
        controller: _name,
        enabled: !_busy,
        decoration: InputDecoration(labelText: context.tr('hostProjectName')),
      ),
      TextField(
        controller: _path,
        enabled: !_busy,
        decoration: InputDecoration(
          labelText: context.tr('hostProjectPath'),
          suffixIcon: IconButton(
            tooltip: context.tr('hostChooseDirectory'),
            onPressed: _busy ? null : _browse,
            icon: const AppIcon('folder'),
          ),
        ),
      ),
      FilledButton(
        onPressed: _busy ? null : _save,
        child: Text(context.tr('save')),
      ),
    ],
  );
}
