import 'dart:async';
import 'dart:collection';
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:flutter/painting.dart';
import 'package:flutter/services.dart';
import 'package:webview_flutter/webview_flutter.dart';

import 'data.dart';
import 'raster.dart';

typedef VisualRender = Future<VisualSvg> Function(VisualRequest request);

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

class VisualRenderer extends ChangeNotifier {
  VisualRenderer({VisualRender? render}) : renderOverride = render;

  VisualRender? renderOverride;
  WebViewController? get controller => _controller;
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
      _cache.remove(jsonEncode(job.request.toJson()))?.future.ignore();
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
              final injected = renderOverride;
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
    await controller.setBackgroundColor(const Color(0x00000000));
    available();
    _controller = controller;
    notifyListeners();
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
    if (!_closed) {
      _controller = null;
      notifyListeners();
    }
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
      entry.future.ignore();
      _evict(entry);
    }
    _cache.clear();
    _cacheBytes = 0;
    _release();
    _controller = null;
    super.dispose();
  }
}
