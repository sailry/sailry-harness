import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:webview_flutter/webview_flutter.dart';

import '../../l10n/strings.dart';
import '../../ui/kit.dart';

class PortPreviewPage extends StatefulWidget {
  const PortPreviewPage({
    super.key,
    required this.uri,
    required this.listening,
  });

  final Uri uri;
  final ValueListenable<bool> listening;

  @override
  State<PortPreviewPage> createState() => _PortPreviewPageState();
}

class _PortPreviewPageState extends State<PortPreviewPage> {
  WebViewController? _controller;
  bool _loading = true;
  bool _failed = false;

  @override
  void initState() {
    super.initState();
    unawaited(_initialize());
  }

  Future<void> _initialize() async {
    try {
      final controller = WebViewController();
      await controller.setJavaScriptMode(JavaScriptMode.unrestricted);
      if (!mounted) return;
      await controller.setNavigationDelegate(
        NavigationDelegate(
          onPageStarted: (_) {
            if (mounted) {
              setState(() {
                _loading = true;
                _failed = false;
              });
            }
          },
          onPageFinished: (_) {
            if (mounted) setState(() => _loading = false);
          },
          onWebResourceError: (error) {
            // A failed subresource must not replace a usable page.
            if (error.isForMainFrame == true) _fail();
          },
          onNavigationRequest: (request) {
            final uri = Uri.tryParse(request.url);
            if (uri != null &&
                (uri.scheme == 'http' || uri.scheme == 'https')) {
              return NavigationDecision.navigate;
            }
            if (mounted && request.isMainFrame) {
              ScaffoldMessenger.of(context).showSnackBar(
                SnackBar(content: Text(context.tr('resourcePreviewLink'))),
              );
            }
            return NavigationDecision.prevent;
          },
        ),
      );
      if (!mounted || !widget.listening.value) return;
      setState(() => _controller = controller);
      await controller.loadRequest(widget.uri);
    } catch (_) {
      _fail();
    }
  }

  void _fail() {
    if (!mounted) return;
    setState(() {
      _loading = false;
      _failed = true;
    });
  }

  Future<void> _reload() async {
    if (_loading || !widget.listening.value) return;
    setState(() {
      _loading = true;
      _failed = false;
    });
    try {
      final controller = _controller;
      if (controller == null) {
        await _initialize();
      } else {
        await controller.reload();
      }
    } catch (_) {
      _fail();
    }
  }

  @override
  Widget build(BuildContext context) => ValueListenableBuilder<bool>(
    valueListenable: widget.listening,
    builder: (context, listening, _) => PageFrame(
      title: context.tr('browser'),
      scroll: false,
      padding: EdgeInsets.zero,
      loading: listening && _loading,
      actions: [
        RoundButton(
          icon: 'refresh',
          tooltip: context.tr('refresh'),
          onPressed: listening && !_loading ? _reload : null,
        ),
      ],
      child: SafeArea(
        top: false,
        child: Stack(
          fit: StackFit.expand,
          children: [
            if (listening && _controller != null)
              WebViewWidget(controller: _controller!),
            if (!listening || _failed)
              ColoredBox(
                color: Theme.of(context).colorScheme.surface,
                child: FailureState(
                  icon: 'globe',
                  message: context.tr(
                    listening
                        ? 'resourcePreviewFailed'
                        : 'resourceForwardStopped',
                  ),
                  onRetry: listening ? _reload : null,
                ),
              ),
          ],
        ),
      ),
    ),
  );
}
