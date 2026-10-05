import 'package:flutter/material.dart';

import '../../l10n/strings.dart';
import '../../runtime/json.dart';
import '../../runtime/session.dart';
import '../../ui/kit.dart';
import '../conversations/live/presentation.dart';

/// Browses the selected execution host using the same Rust API as Desktop.
class HostFilePicker extends StatefulWidget {
  const HostFilePicker({
    super.key,
    required this.host,
    this.directoryOnly = false,
    this.initialPath,
  });
  final HostConnection host;
  final bool directoryOnly;
  final String? initialPath;

  @override
  State<HostFilePicker> createState() => _HostFilePickerState();
}

class _HostFilePickerState extends State<HostFilePicker> {
  final _address = TextEditingController();
  Map<String, dynamic> _listing = {};
  bool _busy = false;
  String? _error;

  Map<String, dynamic> get _directory => object(_listing['directory']);

  @override
  void initState() {
    super.initState();
    _load(widget.initialPath);
  }

  @override
  void dispose() {
    _address.dispose();
    super.dispose();
  }

  Future<void> _load(String? path, {bool more = false}) async {
    if (_busy) return;
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      final response = await widget.host.command('browse_files', {
        'directory': path,
        'after': more ? _directory['next'] : null,
      });
      if (!mounted) return;
      final listing = object(response['data']);
      final directory = object(listing['directory']);
      if (more) {
        directory['entries'] = [
          ...objects(_directory['entries']),
          ...objects(directory['entries']),
        ];
      }
      setState(() {
        _listing = {...listing, 'directory': directory};
        _address.text = text(directory['path']);
      });
    } catch (error) {
      if (mounted) {
        setState(() => _error = failureLabel(error, translate: context.tr));
      }
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  String _path(String name) {
    final separator = text(_listing['separator']);
    final directory = text(_directory['path']);
    return '${directory.endsWith(separator) ? directory : '$directory$separator'}$name';
  }

  @override
  Widget build(BuildContext context) => PageFrame(
    title: context.tr(
      widget.directoryOnly ? 'hostChooseDirectory' : 'hostChooseFile',
    ),
    scroll: false,
    actions: [
      RoundButton(
        icon: 'close',
        tooltip: context.tr('close'),
        onPressed: () => Navigator.pop(context),
      ),
    ],
    child: Column(
      children: [
        TextField(
          controller: _address,
          enabled: !_busy,
          textInputAction: TextInputAction.go,
          decoration: InputDecoration(
            labelText: context.tr('hostProjectPath'),
            prefixIcon: IconButton(
              tooltip: context.tr('hostParentDirectory'),
              icon: const AppIcon('back'),
              onPressed: _busy || _listing['parent'] == null
                  ? null
                  : () => _load(_listing['parent'] as String),
            ),
            suffixIcon: IconButton(
              tooltip: context.tr('hostRefresh'),
              icon: const AppIcon('refresh'),
              onPressed: _busy
                  ? null
                  : () => _load(_address.text.isEmpty ? null : _address.text),
            ),
          ),
          onSubmitted: (value) => _load(value.isEmpty ? null : value),
        ),
        const SizedBox(height: 8),
        if (objects(_listing['locations']).isNotEmpty)
          SingleChildScrollView(
            scrollDirection: Axis.horizontal,
            child: Row(
              children: [
                for (final location in objects(_listing['locations']))
                  TextButton(
                    onPressed: _busy
                        ? null
                        : () => _load(text(location['path'])),
                    child: Text(text(location['name'])),
                  ),
              ],
            ),
          ),
        Expanded(
          child: LoadingOverlay(
            loading: _busy,
            child: _error != null
                ? FailureState(
                    icon: 'folder',
                    message: _error!,
                    onRetry: () =>
                        _load(_address.text.isEmpty ? null : _address.text),
                  )
                : objects(_directory['entries']).isEmpty && !_busy
                ? EmptyState(
                    icon: 'folder',
                    message: context.tr('hostEmptyDirectory'),
                  )
                : ListView(
                    children: [
                      for (final entry in objects(_directory['entries']))
                        ListTile(
                          leading: AppIcon(
                            entry['kind'] == 'directory' ? 'folder' : 'file',
                          ),
                          title: Text(
                            text(entry['name']),
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                          ),
                          enabled:
                              !_busy &&
                              (entry['kind'] == 'directory' ||
                                  !widget.directoryOnly &&
                                      entry['kind'] == 'file'),
                          trailing: entry['kind'] == 'directory'
                              ? const AppIcon('chevron')
                              : null,
                          onTap: () {
                            final path = _path(text(entry['name']));
                            if (entry['kind'] == 'directory') {
                              _load(path);
                            } else {
                              Navigator.pop(context, path);
                            }
                          },
                        ),
                      if (_directory['next'] != null)
                        TextButton(
                          onPressed: _busy
                              ? null
                              : () =>
                                    _load(text(_directory['path']), more: true),
                          child: Text(context.tr('hostLoadMore')),
                        ),
                    ],
                  ),
          ),
        ),
        if (widget.directoryOnly)
          Padding(
            padding: EdgeInsets.only(
              top: 12,
              bottom: MediaQuery.paddingOf(context).bottom + 12,
            ),
            child: SizedBox(
              width: double.infinity,
              child: FilledButton(
                onPressed: _busy || _error != null || _directory['path'] == null
                    ? null
                    : () => Navigator.pop(context, text(_directory['path'])),
                child: Text(context.tr('hostChooseDirectory')),
              ),
            ),
          ),
      ],
    ),
  );
}
