import 'dart:typed_data';

import 'package:flutter/material.dart';

import '../../content/code.dart';
import '../../content/external_file.dart';
import '../../content/image_preview.dart';
import '../../content/images.dart';
import '../../content/paths.dart';
import '../../content/markdown.dart';
import '../../content/transfers.dart';
import '../../l10n/strings.dart';
import '../../runtime/json.dart';
import '../../runtime/notices.dart';
import '../../runtime/session.dart';
import '../../ui/kit.dart';
import '../../ui/theme.dart';
import '../../ui/toast.dart';
import 'file_draft.dart';
import 'files/code_view.dart';

/// Retained by the browser so navigating back never discards a draft or retry ID.
class FileDocument {
  FileDraft? draft;
  String? trashRequest;
}

class ResourceFilePage extends StatefulWidget {
  const ResourceFilePage({
    super.key,
    required this.host,
    required this.worktree,
    required this.path,
    required this.document,
    required this.onOpenFile,
  });
  final HostConnection host;
  final String worktree;
  final String path;
  final FileDocument document;
  final Future<void> Function(String path) onOpenFile;

  @override
  State<ResourceFilePage> createState() => _ResourceFilePageState();
}

class _ResourceFilePageState extends State<ResourceFilePage> {
  final _controller = TextEditingController();
  Uint8List? _image;
  bool _busy = false;
  bool _mutating = false;
  bool _editing = false;

  /// Load failure while nothing is shown; other failures use toasts.
  String? _error;
  FileDraft? get _draft => widget.document.draft;
  String get _name => widget.path.split('/').last;
  String get _directory => splitPath(widget.path).directory;
  String get _mime => mediaType(widget.path);
  bool get _markdown => _mime == 'text/markdown';
  bool get _isImage => _mime.startsWith('image/');
  bool get _isText =>
      _draft != null ||
      _mime.startsWith('text/') ||
      _mime == 'application/json';
  bool get _pending =>
      _draft?.pending != null || widget.document.trashRequest != null;

  @override
  void initState() {
    super.initState();
    _load();
  }

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  Future<void> _run(
    Future<void> Function() action, {
    bool mutating = false,
    bool load = false,
  }) async {
    setState(() {
      _busy = true;
      _mutating = mutating;
      _error = null;
    });
    try {
      await action();
    } catch (error) {
      if (mounted) {
        final message = error is FileOpenFailure
            ? context.tr(error.messageKey)
            : failureText(error, translate: context.tr);
        if (load && _draft == null && _image == null) {
          setState(() => _error = message);
        } else {
          showToast(context, message);
        }
      }
    } finally {
      if (mounted) {
        setState(() {
          _busy = false;
          _mutating = false;
        });
      }
    }
  }

  Future<void> _load({bool reload = false}) => _run(() async {
    if (_isImage) {
      final bytes = await downloadFile(
        widget.host,
        widget.worktree,
        widget.path,
      );
      if (mounted) _image = bytes;
    } else if (_mime != 'application/pdf') {
      if (_draft == null || reload || (!_draft!.dirty && !_pending)) {
        final result = await widget.host.command('read_file', {
          'worktree': widget.worktree,
          'path': widget.path,
        });
        if (!mounted) return;
        if (_draft == null) {
          widget.document.draft = FileDraft(object(result['data']));
        } else {
          _draft!.reload(object(result['data']));
        }
      }
      if (mounted) {
        _controller.text = _draft!.text;
        _editing = _draft!.dirty || _draft!.pending != null;
      }
    }
  }, load: true);

  Future<void> _openLink(Uri uri) async {
    final tree = objects(
      widget.host.snapshot['worktrees'],
    ).where((tree) => tree['id'] == widget.worktree).firstOrNull;
    final path = resourcePath(
      uri,
      tree?['path'] as String?,
      lineReference: true,
      directory: widget.path.contains('/')
          ? widget.path.substring(0, widget.path.lastIndexOf('/'))
          : '',
    );
    if (path == null) {
      showToast(context, context.tr('fileLinkUnavailable'));
      return;
    }
    if (path == widget.path) return;
    await widget.onOpenFile(path);
    if (mounted) await _load();
  }

  Future<bool> _confirm(String title, String message, String action) async =>
      await showAppDialog<bool>(
        context: context,
        builder: (context) => AlertDialog(
          title: Text(title),
          content: Text(message),
          actions: [
            TextButton(
              onPressed: () => Navigator.pop(context, false),
              child: Text(context.tr('cancel')),
            ),
            FilledButton(
              onPressed: () => Navigator.pop(context, true),
              child: Text(action),
            ),
          ],
        ),
      ) ==
      true;

  Future<void> _save() => _run(() async {
    try {
      await _draft!.save(widget.host, widget.worktree, widget.path);
      if (mounted) _editing = false;
    } catch (error) {
      if (mounted) {
        showToast(
          context,
          '${context.tr('resourceSaveError')}\n${failureText(error, translate: context.tr)}',
        );
      }
    }
  }, mutating: true);

  Future<void> _reload() async {
    if (_draft?.dirty == true &&
        !await _confirm(
          context.tr('refresh'),
          context.tr('resourceReloadConfirm'),
          context.tr('discard'),
        )) {
      return;
    }
    if (mounted) await _load(reload: true);
  }

  Future<void> _export({required bool share, Rect? origin}) async {
    if (_draft?.dirty == true) {
      if (!await _confirm(
        context.tr('unsaved'),
        context.tr('fileSaveBeforeShare'),
        context.tr('save'),
      )) {
        return;
      }
      if (!mounted) return;
      await _save();
      if (!mounted || _draft!.dirty || _pending) return;
    }
    if (!mounted) return;
    await _run(() async {
      final bytes = await downloadFile(
        widget.host,
        widget.worktree,
        widget.path,
      );
      if (!mounted) return;
      final file = await cacheExternalFile(bytes, _name);
      if (!mounted) return;
      if (share) {
        await shareFile(file, _name, origin!);
      } else {
        await openFile(file, _name);
      }
    });
  }

  Future<void> _trash() async {
    if (widget.document.trashRequest == null &&
        !await _confirm(
          context.tr('delete'),
          context.tr('fileTrashConfirm').replaceAll('{name}', _name),
          context.tr('delete'),
        )) {
      return;
    }
    if (!mounted) return;
    await _run(() async {
      try {
        final request = widget.document.trashRequest;
        if (request == null) {
          await widget.host.command('trash_entry', {
            'worktree': widget.worktree,
            'path': widget.path,
          });
        } else {
          await widget.host.execute(request);
        }
        widget.document.trashRequest = null;
        if (mounted) Navigator.pop(context, true);
      } on CommandFailure catch (error) {
        widget.document.trashRequest = error.code == 'outcome_unknown'
            ? error.request
            : null;
        rethrow;
      }
    }, mutating: true);
  }

  Future<void> _more(Rect origin) async {
    final action = await showAppSheet<String>(
      context,
      context.tr('more'),
      child: Builder(
        builder: (context) => Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            for (final (key, icon, label, enabled) in [
              (
                'refresh',
                'refresh',
                'refresh',
                (_isText || _isImage) && !_pending,
              ),
              ('share', 'send', 'fileShare', !_pending),
              ('open', 'link', 'fileOpenExternal', !_pending),
              (
                'trash',
                'trash',
                widget.document.trashRequest == null ? 'fileTrash' : 'retry',
                _draft?.pending == null,
              ),
            ])
              if (enabled)
                ListTile(
                  leading: AppIcon(icon),
                  title: Text(context.tr(label)),
                  textColor: key == 'trash'
                      ? Theme.of(context).colorScheme.error
                      : null,
                  onTap: () => Navigator.pop(context, key),
                ),
          ],
        ),
      ),
    );
    if (!mounted || action == null) return;
    switch (action) {
      case 'refresh':
        await _reload();
      case 'share':
        await _export(share: true, origin: origin);
      case 'open':
        await _export(share: false);
      case 'trash':
        await _trash();
    }
  }

  void _edit() {
    if (!_editing) {
      setState(() => _editing = true);
    } else if (_draft!.dirty || _draft!.pending != null) {
      _save();
    } else {
      setState(() => _editing = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return PopScope(
      canPop: !_mutating,
      child: PageFrame(
        loading: _busy,
        title: _name,
        subtitle: _directory.isEmpty
            ? null
            : Text(
                _directory,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(
                  fontFamily: 'monospace',
                  fontSize: 12,
                  color: colors.onSurfaceVariant.withValues(alpha: .65),
                ),
              ),
        titleAccessory: _draft?.dirty == true
            ? Tooltip(
                message: context.tr('unsaved'),
                child: Container(
                  width: 7,
                  height: 7,
                  decoration: BoxDecoration(
                    color: SailryTheme.statusColor(context, StatusTone.warning),
                    shape: BoxShape.circle,
                  ),
                ),
              )
            : null,
        scroll: false,
        padding: EdgeInsets.zero,
        backEnabled: !_mutating,
        actions: [
          if (_isText)
            RoundButton(
              icon: _editing ? 'check' : 'edit',
              tooltip: context.tr(
                _editing
                    ? (_draft?.pending == null ? 'save' : 'retry')
                    : 'edit',
              ),
              onPressed:
                  _busy ||
                      widget.document.trashRequest != null ||
                      _draft?.editable != true
                  ? null
                  : _edit,
            ),
          Builder(
            builder: (context) => RoundButton(
              icon: 'ellipsis',
              tooltip: context.tr('more'),
              onPressed: _busy
                  ? null
                  : () {
                      final box = context.findRenderObject()! as RenderBox;
                      _more(box.localToGlobal(Offset.zero) & box.size);
                    },
            ),
          ),
        ],
        child: SafeArea(
          top: false,
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              if (widget.document.trashRequest != null)
                _Note(context.tr('fileTrashUncertain')),
              if (_draft?.truncated == true)
                _Note(context.tr('resourcePartial')),
              Expanded(child: _body()),
            ],
          ),
        ),
      ),
    );
  }

  Widget _body() {
    if (_error != null) {
      return Padding(
        padding: const EdgeInsets.symmetric(horizontal: 20),
        child: FailureState(
          icon: 'file',
          message: _error!,
          onRetry: _busy ? null : _load,
        ),
      );
    }
    if (_isImage) {
      return _image == null
          ? const SizedBox()
          : ImageViewer(image: MemoryImage(_image!));
    }
    if (!_isText) {
      return Center(
        child: FilledButton(
          onPressed: _busy || _pending ? null : () => _export(share: false),
          child: Text(context.tr('fileOpenExternal')),
        ),
      );
    }
    if (_draft == null) return const SizedBox();
    if (_editing) {
      return Padding(
        padding: const EdgeInsets.symmetric(horizontal: 16),
        child: TextField(
          key: const ValueKey('resource-file-input'),
          controller: _controller,
          enabled: !_busy && !_pending,
          expands: true,
          minLines: null,
          maxLines: null,
          textAlignVertical: TextAlignVertical.top,
          autocorrect: false,
          enableSuggestions: false,
          style: const TextStyle(
            fontFamily: 'monospace',
            fontSize: 14,
            height: 1.6,
          ),
          onChanged: (value) => setState(() => _draft!.text = value),
        ),
      );
    }
    // Markdown reads rendered; editing shows its source.
    if (_markdown) {
      return SingleChildScrollView(
        padding: const EdgeInsets.fromLTRB(20, 12, 20, 12),
        child: MarkdownContent(
          _draft!.text,
          onOpenFile: _openLink,
          imageBuilder: (uri, _, _) => contentImage(
            widget.host,
            widget.worktree,
            uri,
            directory: _directory,
          ),
        ),
      );
    }
    return CodeView(_draft!.text, language: languageFor(widget.path));
  }
}

class _Note extends StatelessWidget {
  const _Note(this.message);
  final String message;

  @override
  Widget build(BuildContext context) => Padding(
    padding: const EdgeInsets.fromLTRB(20, 0, 20, 8),
    child: Text(
      message,
      style: TextStyle(
        fontSize: 12,
        color: Theme.of(context).colorScheme.onSurfaceVariant,
      ),
    ),
  );
}
