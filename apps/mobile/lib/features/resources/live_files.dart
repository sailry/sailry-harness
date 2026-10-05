import 'package:flutter/material.dart';

import '../../l10n/strings.dart';
import '../../content/file_size.dart';
import '../../runtime/json.dart';
import '../../runtime/notices.dart';
import '../../ui/kit.dart';
import 'workspace.dart';
import 'file_page.dart';
import 'file_navigation.dart';
import 'file_location.dart';

class LiveFilesPage extends StatefulWidget {
  const LiveFilesPage({
    super.key,
    this.hostId,
    this.worktreeId,
    this.initialDirectory = '',
  });
  final String? hostId;
  final String? worktreeId;
  final String initialDirectory;
  @override
  State<LiveFilesPage> createState() => _LiveFilesPageState();
}

class _LiveFilesPageState extends State<LiveFilesPage> {
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
        setState(() => _error = failureText(error));
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
      if (mounted && identity == _identity) {
        setState(() => _error = failureText(error));
      }
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
        tr('searchFiles'),
        child: Builder(
          builder: (context) => Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              if (matches.isEmpty)
                EmptyState(icon: 'search', message: tr('noResults')),
              for (final match in matches)
                ListTile(
                  title: Text('${text(match['path'])}:${match['line_number']}'),
                  subtitle: Text(text(match['line'])),
                  onTap: () => Navigator.pop(context, text(match['path'])),
                ),
              if (data['truncated'] == true) Text(tr('resourcePartial')),
            ],
          ),
        ),
      );
      if (mounted && identity == _identity && path != null) await _open(path);
    } catch (error) {
      if (mounted && identity == _identity) {
        setState(() => _error = failureText(error));
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final target = _target;
    final entries = objects(_listing?['entries']);
    final next = _listing?['next'];
    return PageFrame(
      loading: _busy,
      title: tr('files'),
      failure: target != null && !target.host.connected
          ? const HostState(added: true)
          : null,
      empty: target == null
          ? EmptyState(message: tr('resourceNoWorkspace'))
          : entries.isEmpty && !_busy && _error == null
          ? EmptyState(message: tr('resourceEmpty'))
          : null,
      actions: [
        RoundButton(
          icon: 'search',
          tooltip: tr('searchFiles'),
          onPressed: _busy ? null : _search,
        ),
        RoundButton(
          icon: 'refresh',
          tooltip: tr('refresh'),
          onPressed: _busy ? null : _load,
        ),
      ],
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          if (target != null && target.host.connected) ...[
            FileLocation(
              directory: _directory,
              enabled: !_busy && _opening == null,
              onNavigate: _navigate,
            ),
            const SizedBox(height: 8),
            if (_error != null)
              FailureState(icon: 'folder', message: _error!, onRetry: _load)
            else if (entries.isNotEmpty)
              Surface(
                padding: const EdgeInsets.symmetric(horizontal: 14),
                child: Column(
                  children: [
                    for (var i = 0; i < entries.length; i++) ...[
                      if (i > 0) const Divider(),
                      ListTile(
                        leading: AppIcon(
                          entries[i]['kind'] == 'directory' ? 'folder' : 'file',
                        ),
                        title: Text(
                          text(entries[i]['name']),
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                        ),
                        trailing: entries[i]['kind'] == 'directory'
                            ? const AppIcon('chevron', size: 14)
                            : Text(
                                fileSize(entries[i]['size'] as int?),
                                style: Theme.of(context).textTheme.bodySmall
                                    ?.copyWith(
                                      color: Theme.of(
                                        context,
                                      ).colorScheme.onSurfaceVariant,
                                    ),
                              ),
                        onTap: _busy || _opening != null
                            ? null
                            : () {
                                final path = [
                                  _directory,
                                  text(entries[i]['name']),
                                ].where((part) => part.isNotEmpty).join('/');
                                if (entries[i]['kind'] == 'directory') {
                                  _navigate(path);
                                } else {
                                  _open(path);
                                }
                              },
                      ),
                    ],
                  ],
                ),
              ),
            if (_error == null && next != null)
              TextButton(
                onPressed: _busy ? null : () => _load(after: object(next)),
                child: Text(tr('resourceMore')),
              )
            else if (_error == null && _listing?['truncated'] == true)
              Text(tr('resourcePartial')),
          ],
        ],
      ),
    );
  }
}
