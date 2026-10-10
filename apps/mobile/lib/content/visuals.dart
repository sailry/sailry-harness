import 'dart:async';
import 'dart:collection';
import 'dart:convert';
import 'dart:math' as math;

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_svg/flutter_svg.dart';
import 'package:webview_flutter/webview_flutter.dart';

import '../l10n/strings.dart';
import '../ui/kit.dart';
import 'code.dart';
import 'visuals/data.dart';
import 'visuals/image.dart';
import 'visuals/raster.dart';

export 'visuals/data.dart';

typedef VisualRender = Future<VisualSvg> Function(VisualRequest request);

/// A conversation or document owns one lazy, offline rendering surface.
class VisualScope extends StatefulWidget {
  const VisualScope({super.key, required this.child, this.render});
  final Widget child;
  final VisualRender? render;

  static bool contains(BuildContext context) =>
      context.getInheritedWidgetOfExactType<_VisualHost>() != null;

  @override
  State<VisualScope> createState() => _VisualScopeState();
}

class _VisualJob {
  _VisualJob(this.id, this.request);
  final int id;
  final VisualRequest request;
  final completion = Completer<VisualSvg>();
}

class _CachedVisual {
  _CachedVisual(this.future);
  final Future<VisualSvg> future;
  int? bytes;
  VisualSvg? svg;
}

class _VisualScopeState extends State<VisualScope> {
  final _cache = <String, _CachedVisual>{};
  var _cacheBytes = 0;
  final _queue = Queue<_VisualJob>();
  WebViewController? _controller;
  Future<void>? _ready;
  Completer<void>? _loaded;
  _VisualJob? _active;
  Timer? _deadline;
  Completer<void>? _interruption;
  var _serial = 0;
  var _generation = 0;
  var _draining = false;
  var _closed = false;

  Future<VisualSvg> render(VisualRequest request) {
    final key = jsonEncode(request.toJson());
    // A queued render for an earlier global theme has no current consumer.
    for (final job
        in _queue
            .where(
              (job) =>
                  job.request.kind == request.kind &&
                  job.request.source == request.source &&
                  job.request.fontSize == request.fontSize &&
                  (job.request.dark != request.dark ||
                      job.request.background != request.background ||
                      job.request.accent != request.accent),
            )
            .toList()) {
      _queue.remove(job);
      _cache.remove(jsonEncode(job.request.toJson()));
      job.completion.completeError(StateError('Visual theme changed'));
    }
    final cached = _cache.remove(key);
    if (cached != null) {
      _cache[key] = cached;
      return cached.future;
    }
    _trim(127);
    if (_cache.length >= 128) {
      return Future.error(StateError('Visual renderer is busy'));
    }
    late final _CachedVisual entry;
    final result =
        Future<VisualSvg>.sync(() {
              request.validate();
              if (_closed) throw StateError('Visual renderer closed');
              final injected = widget.render;
              if (injected != null) return injected(request);
              if (_queue.length >= 256) {
                throw StateError('Visual renderer is busy');
              }
              final job = _VisualJob(++_serial, request);
              _queue.add(job);
              unawaited(_drain());
              return job.completion.future;
            })
            .then((svg) {
              svg.validate();
              return svg;
            })
            .then(
              (svg) {
                entry.svg = svg;
                // Two bytes per code unit conservatively account for retained Dart strings.
                entry.bytes =
                    (svg.source.length + key.length) * 2 +
                    (svg.png?.length ?? 0);
                if (_cache[key] == entry) {
                  _cacheBytes += entry.bytes!;
                  _trim(128);
                }
                return svg;
              },
              onError: (Object error, StackTrace stack) {
                entry.bytes = key.length * 2;
                if (_cache[key] == entry) {
                  _cacheBytes += entry.bytes!;
                  _trim(128);
                }
                Error.throwWithStackTrace(error, stack);
              },
            );
    entry = _CachedVisual(result);
    _cache[key] = entry;
    return result;
  }

  void _trim(int entries) {
    while (_cache.length > entries || _cacheBytes > 16 * 1024 * 1024) {
      final oldest = _cache.entries
          .where((entry) => entry.value.bytes != null)
          .firstOrNull;
      if (oldest == null) break;
      _cache.remove(oldest.key);
      _cacheBytes -= oldest.value.bytes!;
      _evict(oldest.value);
    }
  }

  void _evict(_CachedVisual entry) {
    final bytes = entry.svg?.png;
    if (bytes != null) {
      unawaited(MemoryImage(bytes).evict().then<void>((_) {}));
    }
  }

  Future<void> _initialize() async {
    final generation = ++_generation;
    void available() {
      if (_closed || generation != _generation) {
        throw StateError('Visual renderer closed');
      }
    }

    final scripts = await Future.wait([
      for (final name in ['config', 'mermaid', 'mathjax', 'fonts', 'render'])
        rootBundle.loadString('assets/visuals/$name.js', cache: false),
    ]);
    available();
    final controller = WebViewController();
    await controller.setBackgroundColor(Colors.transparent);
    available();
    setState(() => _controller = controller);
    await controller.setJavaScriptMode(JavaScriptMode.unrestricted);
    available();
    await controller.addJavaScriptChannel(
      'SailryVisual',
      onMessageReceived: _accept,
    );
    available();
    final loaded = _loaded = Completer<void>();
    final loading = loaded.future.timeout(const Duration(seconds: 30));
    unawaited(loading.catchError((Object _) {}));
    await controller.setNavigationDelegate(
      NavigationDelegate(
        onNavigationRequest: (request) => request.url == 'about:blank'
            ? NavigationDecision.navigate
            : NavigationDecision.prevent,
        onPageFinished: (_) {
          if (!loaded.isCompleted) loaded.complete();
        },
        onWebResourceError: (error) {
          if (!loaded.isCompleted) {
            loaded.completeError(StateError(error.description));
          }
        },
      ),
    );
    available();
    // Inline scripts have no source URL. MathJax resolves its bundled speech
    // paths during boot, which otherwise throws against about:blank.
    scripts[0] +=
        '\nMathJax.loader.paths = {mathjax:"sailry-visual://localhost"};';
    // Keep font registration and preLoaded in one script task, before MathJax's
    // loader microtasks can request a separate fonts.js resource.
    final script = [...scripts, diagramRasterizer]
        .join('\n')
        .replaceAll(RegExp('</script', caseSensitive: false), r'<\/script');
    final page =
        '''<!doctype html><html><head><meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data:; connect-src 'none'; font-src 'none'; frame-src 'none'; object-src 'none'; base-uri 'none'">
<style>html,body{width:640px;min-height:480px;margin:0;background:transparent!important}body{opacity:0}</style>
<script>$script</script>
</head><body><main id="surface"></main></body></html>''';
    await controller.loadHtmlString(page);
    available();
    await loading;
    available();
  }

  Future<void> _drain() async {
    if (_draining || _closed) return;
    _draining = true;
    try {
      await (_ready ??= _initialize().timeout(const Duration(seconds: 30)));
      while (!_closed && _queue.isNotEmpty) {
        final job = _active = _queue.removeFirst();
        final interruption = _interruption = Completer<void>();
        _deadline = Timer(const Duration(seconds: 30), () {
          if (!interruption.isCompleted) {
            interruption.completeError(
              TimeoutException('Visual rendering timed out'),
            );
          }
        });
        try {
          await Future.any<void>([
            Future.wait<void>([
              _controller!.runJavaScript(
                'Promise.resolve().then(()=>window.sailryVisual(${jsonEncode(job.request.toJson())})).then('
                'result=>SailryVisual.postMessage(JSON.stringify({id:${job.id},result,error:null})),'
                'error=>SailryVisual.postMessage(JSON.stringify({id:${job.id},result:null,error:String(error?.message||error).slice(0,2048)})))',
              ),
              job.completion.future.then<void>((_) {}),
            ], eagerError: true),
            interruption.future,
          ]);
        } catch (error, stack) {
          if (!job.completion.isCompleted) {
            job.completion.completeError(error, stack);
          }
          if (error is TimeoutException) {
            // Release the surface before another job can touch its shared DOM.
            _release();
            while (_queue.isNotEmpty) {
              _queue.removeFirst().completion.completeError(error, stack);
            }
            break;
          }
        } finally {
          _deadline?.cancel();
          _deadline = null;
          _interruption = null;
          _active = null;
        }
      }
    } catch (error, stack) {
      _release();
      while (_queue.isNotEmpty) {
        _queue.removeFirst().completion.completeError(error, stack);
      }
    } finally {
      _draining = false;
    }
  }

  void _release() {
    _generation++;
    final controller = _controller;
    if (!_closed) setState(() => _controller = null);
    _ready = null;
    final loaded = _loaded;
    _loaded = null;
    if (loaded != null && !loaded.isCompleted) loaded.complete();
    if (controller != null) {
      unawaited(
        controller
            .removeJavaScriptChannel('SailryVisual')
            .catchError((Object _) {}),
      );
    }
  }

  void _accept(JavaScriptMessage event) {
    // JSON may escape each source byte as six characters (for example \u0000).
    if (_closed ||
        utf8.encode(event.message).length > 6 * 2097152 + 5592408 + 4096) {
      return;
    }
    final job = _active;
    if (job == null || job.completion.isCompleted) return;
    try {
      final message = jsonDecode(event.message) as Map<String, dynamic>;
      if (message['id'] != job.id) return;
      if (message['error'] != null) {
        throw StateError(message['error'].toString());
      }
      final svg = VisualSvg.fromJson(message['result'] as Map<String, dynamic>);
      svg.validate();
      if (job.request.kind == VisualKind.mermaid && svg.png == null) {
        throw const FormatException(
          'Diagram rasterization did not produce PNG',
        );
      }
      job.completion.complete(svg);
    } catch (error, stack) {
      job.completion.completeError(error, stack);
    }
  }

  @override
  void dispose() {
    _closed = true;
    _deadline?.cancel();
    final interruption = _interruption;
    if (interruption != null && !interruption.isCompleted) {
      interruption.completeError(StateError('Visual renderer closed'));
    }
    final jobs = [?_active, ..._queue];
    for (final job in jobs) {
      if (!job.completion.isCompleted) {
        job.completion.completeError(StateError('Visual renderer closed'));
      }
    }
    _queue.clear();
    for (final entry in _cache.values) {
      _evict(entry);
    }
    _cache.clear();
    _cacheBytes = 0;
    _release();
    _controller = null;
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => _VisualHost(
    owner: this,
    child: Stack(
      children: [
        widget.child,
        if (_controller != null)
          // UIKit sizes the native view during composition. Keep it composed;
          // the transparent page hides its DOM while preserving SVG layout.
          Positioned(
            left: 0,
            top: 0,
            width: 640,
            height: 480,
            child: IgnorePointer(
              child: ExcludeSemantics(
                child: WebViewWidget(controller: _controller!),
              ),
            ),
          ),
      ],
    ),
  );
}

class _VisualHost extends InheritedWidget {
  const _VisualHost({required this.owner, required super.child});
  final _VisualScopeState owner;
  @override
  bool updateShouldNotify(_VisualHost oldWidget) => owner != oldWidget.owner;
}

class MarkdownVisual extends StatelessWidget {
  const MarkdownVisual({
    super.key,
    required this.kind,
    required this.source,
    this.original,
    this.style,
  });
  final VisualKind kind;
  final String source;
  final String? original;
  final TextStyle? style;

  @override
  Widget build(BuildContext context) {
    final scope = context.dependOnInheritedWidgetOfExactType<_VisualHost>();
    if (scope == null) return VisualScope(child: this);
    final theme = Theme.of(context);
    final colors = theme.colorScheme;
    String color(Color value) =>
        '#${value.toARGB32().toRadixString(16).padLeft(8, '0').substring(2)}';
    final request = VisualRequest(
      kind: kind,
      source: source,
      dark: theme.brightness == Brightness.dark,
      foreground: color(style?.color ?? colors.onSurface),
      background: color(colors.surface),
      accent: color(colors.primary),
      fontSize: (style?.fontSize ?? 14).clamp(8, 96).toDouble(),
    );
    final rendered = scope.owner.render(request);
    return FutureBuilder<VisualSvg>(
      key: ObjectKey(rendered),
      future: rendered,
      builder: (context, snapshot) {
        final svg = snapshot.data;
        if (svg == null) {
          return kind == VisualKind.inlineMath
              ? SelectableText(original ?? source, style: style)
              : CodeBlock(
                  original ?? source,
                  key: ValueKey(
                    'visual-source-${snapshot.connectionState.name}',
                  ),
                  language: kind == VisualKind.mermaid ? 'mermaid' : 'latex',
                );
        }
        return LayoutBuilder(
          builder: (context, constraints) {
            final available = constraints.hasBoundedWidth
                ? constraints.maxWidth
                : MediaQuery.sizeOf(context).width;
            final scale = math.min(1.0, available / svg.width);
            final image = Semantics(
              label: original ?? source,
              image: true,
              child: svg.png == null
                  ? SvgPicture(
                      FormulaLoader(svg.source),
                      width: svg.width * scale,
                      height: svg.height * scale,
                    )
                  : Image.memory(
                      svg.png!,
                      width: svg.width * scale,
                      height: svg.height * scale,
                      fit: BoxFit.contain,
                    ),
            );
            if (kind == VisualKind.inlineMath) {
              return GestureDetector(
                onTap: () => showAppSheet(
                  context,
                  context.tr('codePlain'),
                  child: CodeBlock(original ?? source, language: 'latex'),
                ),
                child: Baseline(
                  baseline: (svg.baseline ?? svg.height) * scale,
                  baselineType: TextBaseline.alphabetic,
                  child: image,
                ),
              );
            }
            return Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Align(
                  alignment: Alignment.centerRight,
                  child: CopyTextButton(original ?? source),
                ),
                Align(alignment: Alignment.centerLeft, child: image),
              ],
            );
          },
        );
      },
    );
  }
}
