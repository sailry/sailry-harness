import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:webview_flutter/webview_flutter.dart';

import '../../../content/code.dart';
import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../ui/toast.dart';
import 'content.dart';
import 'submission.dart';

class ArtifactSurface extends StatefulWidget {
  const ArtifactSurface({
    super.key,
    required this.content,
    required this.connected,
    this.actions,
    this.onReady,
    this.onEdited,
    this.readonly = false,
  });

  final Map<String, dynamic> content;
  final bool connected;
  final ArtifactActions? actions;
  final VoidCallback? onReady;
  final VoidCallback? onEdited;
  final bool readonly;

  @override
  State<ArtifactSurface> createState() => ArtifactSurfaceState();
}

/// A bounded offline platform surface executes only the captured package view.
class ArtifactSurfaceState extends State<ArtifactSurface> {
  WebViewController? _controller;
  Timer? _deadline;
  bool _started = false;
  bool _ready = false;
  bool _failed = false;
  bool _closed = false;
  bool _sending = false;
  String? _pendingForm;
  Completer<String>? _export;
  int _exportId = 0;
  Map<String, String> _theme = {};

  bool get writable =>
      !widget.readonly &&
      widget.content['latest_revision'] ==
          object(widget.content['artifact'])['revision'] &&
      widget.connected &&
      widget.actions?.writable() == true &&
      !widget.actions!.pending();

  @override
  void initState() {
    super.initState();
    widget.actions?.addListener(_changed);
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    final next = artifactTheme(context);
    if (!_started) {
      _started = true;
      _theme = next;
      unawaited(_initialize());
    } else if (jsonEncode(next) != jsonEncode(_theme)) {
      _theme = next;
      if (_ready) {
        unawaited(_run('window.sailryDocument.retheme(${jsonEncode(next)})'));
      }
    }
  }

  @override
  void didUpdateWidget(ArtifactSurface oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.actions != widget.actions) {
      oldWidget.actions?.removeListener(_changed);
      widget.actions?.addListener(_changed);
    }
    if (oldWidget.connected != widget.connected ||
        oldWidget.actions != widget.actions ||
        oldWidget.readonly != widget.readonly) {
      _changed();
    }
  }

  void _changed() {
    if (_closed || !mounted) return;
    setState(() {});
    if (_ready) unawaited(_run('window.sailryDocument.setWritable($writable)'));
  }

  Future<void> _initialize() async {
    try {
      final background = Theme.of(context).colorScheme.surface;
      final page = await artifactPage(widget.content, _theme, {
        'contents': context.tr('artifactContents'),
        'submit': context.tr('send'),
        'zoom_in': context.tr('artifactZoomIn'),
        'zoom_out': context.tr('artifactZoomOut'),
        'close': context.tr('close'),
      }, writable: writable);
      if (_closed) return;
      final controller = WebViewController();
      await controller.setBackgroundColor(background);
      if (_closed) return;
      setState(() => _controller = controller);
      await controller.setJavaScriptMode(JavaScriptMode.unrestricted);
      if (_closed) return;
      await controller.addJavaScriptChannel(
        'SailryDocument',
        onMessageReceived: _accept,
      );
      if (_closed) {
        await controller.removeJavaScriptChannel('SailryDocument');
        return;
      }
      await controller.setNavigationDelegate(
        NavigationDelegate(
          onNavigationRequest: (request) =>
              request.url == 'about:blank' ||
                  request.url.startsWith('about:blank#')
              ? NavigationDecision.navigate
              : NavigationDecision.prevent,
          onWebResourceError: (error) {
            if (error.isForMainFrame == true) _fail();
          },
        ),
      );
      if (_closed) return;
      _deadline = Timer(const Duration(seconds: 30), _fail);
      await controller.loadHtmlString(page, baseUrl: 'about:blank');
    } catch (_) {
      _fail();
    }
  }

  Future<void> _run(String script) async {
    if (_closed || !_ready) return;
    try {
      await _controller!
          .runJavaScript(script)
          .timeout(const Duration(seconds: 30));
    } catch (_) {
      _fail();
    }
  }

  void _fail() {
    if (_closed || !mounted || _failed) return;
    _deadline?.cancel();
    _ready = false;
    setState(() => _failed = true);
    if (_export case final pending? when !pending.isCompleted) {
      pending.completeError(StateError('Artifact renderer unavailable'));
    }
    showToast(context, context.tr('resourcePreviewFailed'));
    widget.onReady?.call();
  }

  void _accept(JavaScriptMessage message) {
    if (_closed || !mounted || message.message.length > 6 * 4 * 1024 * 1024) {
      return;
    }
    try {
      final event = object(jsonDecode(message.message));
      switch (event['kind']) {
        case 'ready':
          _deadline?.cancel();
          setState(() => _ready = true);
          _changed();
          unawaited(
            _run('window.sailryDocument.retheme(${jsonEncode(_theme)})'),
          );
          widget.onReady?.call();
        case 'error':
          _fail();
        case 'edited':
          final artifact = object(widget.content['artifact']);
          if (event['artifact'] == artifact['id'] &&
              event['revision'] == artifact['revision'] &&
              event['session'] == artifact['session']) {
            widget.onEdited?.call();
          }
        case 'required':
        case 'invalid_form':
          showToast(context, context.tr('artifactFormRequired'));
        case 'submit':
          if (_ready && writable && !_sending) unawaited(_submit(event));
        case 'export':
          if (event['id'] == _exportId &&
              _export != null &&
              !_export!.isCompleted) {
            final html = text(event['html']);
            if (utf8.encode(html).length > 4 * 1024 * 1024 || html.isEmpty) {
              _export!.completeError(
                const FormatException('Invalid artifact export'),
              );
            } else {
              _export!.complete(html);
            }
          }
        case 'export_error':
          if (event['id'] == _exportId &&
              _export != null &&
              !_export!.isCompleted) {
            _export!.completeError(StateError('Artifact export failed'));
          }
      }
    } catch (_) {
      if (mounted) showToast(context, context.tr('conversationFailed'));
    }
  }

  Future<void> _submit(Map<String, dynamic> event) async {
    final artifact = object(widget.content['artifact']);
    final form = text(event['form']);
    try {
      final heading = context
          .tr('artifactReply')
          .replaceAll('{title}', text(artifact['title']))
          .replaceAll('{revision}', '${artifact['revision']}')
          .replaceAll('{form}', form)
          .replaceAll('{id}', text(artifact['id']));
      final message = artifactAnswers(event, artifact, heading);
      setState(() => _sending = true);
      _pendingForm = form;
      await widget.actions!.submit(message);
      if (_closed) return;
      await _run('window.sailryDocument.submitted(${jsonEncode(form)},true)');
      _pendingForm = null;
    } catch (error) {
      if (_closed) return;
      if (widget.actions?.pending() != true) {
        await _run(
          'window.sailryDocument.submitted(${jsonEncode(form)},false)',
        );
        _pendingForm = null;
      }
      if (mounted) {
        showToast(
          context,
          context.tr(
            widget.actions?.pending() == true
                ? 'conversationUnknown'
                : 'conversationFailed',
          ),
        );
      }
    } finally {
      if (!_closed && mounted) setState(() => _sending = false);
    }
  }

  Future<void> _check() async {
    final actions = widget.actions;
    if (actions == null || _sending) return;
    setState(() => _sending = true);
    try {
      final accepted = await actions.check();
      if (_closed) return;
      if (!actions.pending() && _pendingForm != null) {
        await _run(
          'window.sailryDocument.submitted(${jsonEncode(_pendingForm)},$accepted)',
        );
        _pendingForm = null;
      }
    } finally {
      if (!_closed && mounted) setState(() => _sending = false);
    }
  }

  Future<String> exportHtml() async {
    if (!_ready || _closed || _export != null) {
      throw StateError('Artifact renderer is not ready');
    }
    final result = _export = Completer<String>();
    final id = ++_exportId;
    try {
      await _run(
        'Promise.resolve(window.sailryDocument.readonlyExport()).then('
        'html=>SailryDocument.postMessage(JSON.stringify({kind:"export",id:$id,html})),'
        '()=>SailryDocument.postMessage(JSON.stringify({kind:"export_error",id:$id})))',
      );
      final markup = await result.future.timeout(const Duration(seconds: 30));
      final branding = await Future.wait([
        rootBundle.loadString('assets/branding/sailry-mark.svg'),
        rootBundle.loadString('assets/branding/sailry-site-qr.svg'),
      ]);
      return exportedArtifact(
        markup,
        widget.content,
        _theme,
        logo: branding[0],
        qr: branding[1],
      );
    } finally {
      _export = null;
    }
  }

  @override
  void dispose() {
    _closed = true;
    widget.actions?.removeListener(_changed);
    _deadline?.cancel();
    if (_export case final pending? when !pending.isCompleted) {
      pending.completeError(StateError('Artifact renderer closed'));
    }
    final controller = _controller;
    if (controller != null) {
      unawaited(
        controller
            .removeJavaScriptChannel('SailryDocument')
            .catchError((Object _) {}),
      );
    }
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => Column(
    children: [
      if (widget.actions?.pending() == true)
        Align(
          alignment: Alignment.centerRight,
          child: TextButton(
            onPressed: _sending || !widget.connected ? null : _check,
            child: Text(context.tr('conversationCheckResult')),
          ),
        ),
      Expanded(
        child: _failed
            ? SingleChildScrollView(
                child: CodeBlock(
                  text(widget.content['html']),
                  language: 'html',
                ),
              )
            : Stack(
                fit: StackFit.expand,
                children: [
                  if (_controller != null)
                    WebViewWidget(controller: _controller!),
                  if (!_ready) const Center(child: CircularProgressIndicator()),
                ],
              ),
      ),
    ],
  );
}
