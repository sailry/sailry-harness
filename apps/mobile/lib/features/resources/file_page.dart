import 'dart:typed_data';

import 'package:flutter/material.dart';

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
import 'file_draft.dart';

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
  String? _error;
  FileDraft? get _draft => widget.document.draft;
  String get _name => widget.path.split('/').last;
  String get _mime => mediaType(widget.path);
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
        setState(
          () => _error = error is FileOpenFailure
              ? context.tr(error.messageKey)
              : failureText(error, translate: context.tr),
        );
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
  });

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
      setState(() => _error = context.tr('fileLinkUnavailable'));
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
        _error =
            '${context.tr('resourceSaveError')}\n${failureText(error, translate: context.tr)}';
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

  @override
  Widget build(BuildContext context) => PopScope(
    canPop: !_mutating,
    child: PageFrame(
      loading: _busy,
      title: _name,
      scroll: false,
      backEnabled: !_mutating,
      actions: [
        if (_isText || _isImage)
          RoundButton(
            icon: 'refresh',
            tooltip: context.tr('refresh'),
            onPressed: _busy || _pending ? null : _reload,
          ),
        if (_isText)
          RoundButton(
            icon: _editing ? 'check' : 'edit',
            tooltip: context.tr(
              _editing ? (_draft?.pending == null ? 'save' : 'retry') : 'edit',
            ),
            onPressed:
                _busy ||
                    widget.document.trashRequest != null ||
                    _draft?.editable != true
                ? null
                : _editing
                ? (_draft!.dirty || _draft!.pending != null ? _save : null)
                : () => setState(() => _editing = true),
          ),
        Builder(
          builder: (context) => RoundButton(
            icon: 'send',
            tooltip: context.tr('send'),
            onPressed: _busy || _pending
                ? null
                : () {
                    final box = context.findRenderObject()! as RenderBox;
                    _export(
                      share: true,
                      origin: box.localToGlobal(Offset.zero) & box.size,
                    );
                  },
          ),
        ),
        RoundButton(
          icon: 'trash',
          tooltip: context.tr(
            widget.document.trashRequest == null ? 'delete' : 'retry',
          ),
          onPressed: _busy || _draft?.pending != null ? null : _trash,
        ),
      ],
      child: SafeArea(
        top: false,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            if (_error != null)
              Text(
                _error!,
                style: TextStyle(color: Theme.of(context).colorScheme.error),
              ),
            if (widget.document.trashRequest != null)
              Text(context.tr('fileTrashUncertain')),
            if (_draft?.dirty == true || _editing)
              Row(
                children: [
                  if (_draft?.dirty == true)
                    Expanded(child: Text(context.tr('unsaved')))
                  else
                    const Spacer(),
                  if (_editing)
                    TextButton(
                      onPressed: _busy || _pending
                          ? null
                          : () => setState(() => _editing = false),
                      child: Text(context.tr('preview')),
                    ),
                ],
              ),
            if (_draft?.truncated == true) Text(context.tr('resourcePartial')),
            Expanded(child: _body()),
          ],
        ),
      ),
    ),
  );

  Widget _body() {
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
      return TextField(
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
      );
    }
    return SingleChildScrollView(
      padding: const EdgeInsets.symmetric(vertical: 12),
      child: _mime == 'text/markdown'
          ? MarkdownContent(
              _draft!.text,
              onOpenFile: _openLink,
              imageBuilder: (uri, _, _) => contentImage(
                widget.host,
                widget.worktree,
                uri,
                directory: widget.path.contains('/')
                    ? widget.path.substring(0, widget.path.lastIndexOf('/'))
                    : '',
              ),
            )
          : SelectableText(
              _draft!.text,
              style: const TextStyle(
                fontFamily: 'monospace',
                fontSize: 14,
                height: 1.7,
              ),
            ),
    );
  }
}
