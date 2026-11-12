import 'package:flutter/material.dart';

import '../../l10n/strings.dart';
import '../../content/file_size.dart';
import '../../runtime/json.dart';
import '../../runtime/notices.dart';
import '../../ui/kit.dart';
import 'workspace.dart';
import 'file_page.dart';
import 'file_navigation.dart';
import 'files/breadcrumb.dart';
import 'files/entry_data.dart';
import 'files/entry_row.dart';

class FilesPage extends StatefulWidget {
  const FilesPage({
    super.key,
    this.hostId,
    this.worktreeId,
    this.initialDirectory = '',
  });
  final String? hostId;
  final String? worktreeId;
  final String initialDirectory;
  @override
  State<FilesPage> createState() => _FilesPageState();
}

class _FilesPageState extends State<FilesPage> {
  ResourceTarget? _target;
  String? _identity;
  late String _directory = widget.initialDirectory;
  Map<String, dynamic>? _listing;
  final _documents = <String, FileDocument>{};
  String? _error;
  bool _busy = false;
  String? _opening;
  int _generation = 0;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    final target = resourceTarget(context, widget.hostId, widget.worktreeId);
    final identity = target == null
        ? null
        : '${target.host.id}/${target.worktree['id']}';
    _target = target;
    if (_identity != identity) {
      _directory = _identity == null ? widget.initialDirectory : '';
      _identity = identity;
      _listing = null;
      _opening = null;
      _generation++;
      if (target != null) _load();
    }
  }

  Future<void> _load({Map<String, dynamic>? after}) async {
    final target = _target;
    if (target == null) return;
    final generation = ++_generation;
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      final output = await target.host.command('list_directory', {
        'worktree': target.worktree['id'],
        'path': _directory,
        'after': after,
      });
      if (!mounted || generation != _generation) return;
      final listing = object(output['data']);
      setState(() {
        if (after != null && _listing != null) {
          listing['entries'] = [
            ...objects(_listing!['entries']),
            ...objects(listing['entries']),
          ];
        }
        _listing = listing;
      });
    } catch (error) {
      if (mounted && generation == _generation) {
        setState(() => _error = failureText(error, translate: context.tr));
      }
    } finally {
      if (mounted && generation == _generation) setState(() => _busy = false);
    }
  }

  void _navigate(String directory) {
    if (_busy || _opening != null) return;
    setState(() {
      _directory = directory;
      _listing = null;
    });
    _load();
  }

  Future<void> _open(String path) async {
    final target = _target;
    if (target == null || _opening != null) return;
    final identity = _identity;
    final worktree = text(target.worktree['id']);
    setState(() => _opening = path);
    try {
      await openResourceFile(
        context,
        target.host,
        worktree,
        path,
        documents: _documents,
      );
      if (!mounted || identity != _identity) return;
      await _load();
    } catch (error) {
      if (mounted && identity == _identity) showFailure(context, error);
    } finally {
      if (mounted && identity == _identity) setState(() => _opening = null);
    }
  }

  Future<void> _search() async {
    final target = _target;
    if (target == null) return;
    final identity = _identity;
    final query = await askResourceText(context, 'searchFiles');
    if (!mounted || query == null || identity != _identity) return;
    try {
      final result = await target.host.command('search_files', {
        'worktree': target.worktree['id'],
        'options': {
          'query': query,
          'regex': false,
          'case_sensitive': false,
          'globs': <String>[],
        },
      });
      if (!mounted || identity != _identity) return;
      final data = object(result['data']);
      final matches = objects(data['matches']);
      final path = await showAppSheet<String>(
        context,
        context.tr('searchFiles'),
        child: Builder(
          builder: (context) => Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              if (matches.isEmpty)
                EmptyState(icon: 'search', message: context.tr('noResults')),
              for (final match in matches)
                ListTile(
                  title: Text('${text(match['path'])}:${match['line_number']}'),
                  subtitle: Text(text(match['line'])),
                  onTap: () => Navigator.pop(context, text(match['path'])),
                ),
              if (data['truncated'] == true)
                Text(context.tr('resourcePartial')),
            ],
          ),
        ),
      );
      if (mounted && identity == _identity && path != null) await _open(path);
    } catch (error) {
      if (mounted && identity == _identity) showFailure(context, error);
    }
  }

  @override
  Widget build(BuildContext context) {
    final target = _target;
    final entries = objects(_listing?['entries']);
    final next = _listing?['next'];
    final enabled = !_busy && _opening == null;
    final dim = Theme.of(
      context,
    ).colorScheme.onSurfaceVariant.withValues(alpha: .65);
    return PageFrame(
      loading: _busy,
      title: context.tr('files'),
      failure: target != null && !target.host.connected
          ? HostState(added: true, failure: target.host.error)
          : null,
      empty: target == null
          ? EmptyState(message: context.tr('resourceNoWorkspace'))
          : entries.isEmpty && !_busy && _error == null
          ? EmptyState(message: context.tr('resourceEmpty'))
          : null,
      actions: [
        RoundButton(
          icon: 'refresh',
          tooltip: context.tr('refresh'),
          onPressed: _busy ? null : _load,
        ),
      ],
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          if (target != null && target.host.connected) ...[
            SearchBarButton(
              label: context.tr('searchFiles'),
              onTap: _busy ? null : _search,
            ),
            const SizedBox(height: 12),
            Breadcrumb(
              directory: _directory,
              root: worktreeLabel(target.worktree).isEmpty
                  ? context.tr('resourceRoot')
                  : worktreeLabel(target.worktree),
              enabled: enabled,
              onNavigate: _navigate,
            ),
            const SizedBox(height: 8),
            if (_error != null)
              FailureState(icon: 'folder', message: _error!, onRetry: _load)
            else
              for (final (index, entry) in entries.indexed)
                FileEntryRow(
                  name: text(entry['name']),
                  directory: entry['kind'] == 'directory',
                  size: fileSize(entry['size'] as int?),
                  modified: entryModifiedLabel(context, entry),
                  divider: index < entries.length - 1,
                  onTap: enabled
                      ? () {
                          final path = [
                            _directory,
                            text(entry['name']),
                          ].where((part) => part.isNotEmpty).join('/');
                          if (entry['kind'] == 'directory') {
                            _navigate(path);
                          } else {
                            _open(path);
                          }
                        }
                      : null,
                ),
            if (_error == null && next != null)
              TextButton(
                onPressed: _busy ? null : () => _load(after: object(next)),
                child: Text(context.tr('resourceMore')),
              )
            else if (_error == null && _listing?['truncated'] == true)
              Padding(
                padding: const EdgeInsets.only(top: 8),
                child: Text(
                  context.tr('resourcePartial'),
                  style: TextStyle(fontSize: 12, color: dim),
                ),
              ),
          ],
        ],
      ),
    );
  }
}
