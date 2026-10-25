import 'dart:async';
import 'dart:convert';

import 'package:fake_async/fake_async.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/painting.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/content/visuals/data.dart';
import 'package:sailry_mobile/content/visuals/renderer.dart';
import 'package:webview_flutter_platform_interface/webview_flutter_platform_interface.dart';

import 'support/webview.dart';

const testSvg = VisualSvg(
  source:
      '<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><path d="M0 0L20 10"/></svg>',
  width: 20,
  height: 10,
  baseline: 8,
);
const png =
    'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg==';

VisualRequest request(
  String source, {
  bool dark = false,
  VisualKind kind = VisualKind.inlineMath,
}) => VisualRequest(
  kind: kind,
  source: source,
  dark: dark,
  foreground: dark ? '#ffffff' : '#000000',
  background: dark ? '#000000' : '#ffffff',
  accent: '#123456',
  fontSize: 14,
);

class VisualPlatform extends BrowserPlatform {
  final controllers = <VisualController>[];
  bool hold = false;
  bool rejectLoad = false;
  bool holdDispatch = false;
  bool replyBeforeHold = false;
  bool holdMode = false;
  String? output;

  @override
  PlatformWebViewController createPlatformWebViewController(
    PlatformWebViewControllerCreationParams params,
  ) {
    final created = VisualController(params, this);
    controllers.add(created);
    return controller = created;
  }
}

class VisualController extends BrowserController {
  VisualController(super.params, this.platform);
  final VisualPlatform platform;
  JavaScriptChannelParams? channel;
  final pages = <String>[];
  final calls = <String>[];
  Color? background;
  int removals = 0;
  final mode = Completer<void>();

  @override
  Future<void> setBackgroundColor(Color color) async => background = color;

  @override
  Future<void> setJavaScriptMode(JavaScriptMode value) async {
    await super.setJavaScriptMode(value);
    if (platform.holdMode) await mode.future;
  }

  @override
  Future<void> addJavaScriptChannel(JavaScriptChannelParams params) async =>
      channel = params;
  @override
  Future<void> removeJavaScriptChannel(String name) async {
    removals++;
    channel = null;
  }

  @override
  Future<void> loadHtmlString(String html, {String? baseUrl}) async {
    if (platform.rejectLoad) throw StateError('Native load failed');
    pages.add(html);
    platform.navigation.finished('about:blank');
  }

  @override
  Future<void> runJavaScript(String javaScript) async {
    calls.add(javaScript);
    if (platform.replyBeforeHold) reply(javaScript);
    if (platform.holdDispatch) await Completer<void>().future;
    if (!platform.hold) reply(javaScript);
  }

  void reply(String script) {
    final id = int.parse(RegExp(r'id:(\d+)').firstMatch(script)![1]!);
    channel?.onMessageReceived(
      JavaScriptMessage(
        message: jsonEncode({
          'id': id,
          'error': null,
          'result': {
            'source': platform.output ?? testSvg.source,
            'width': testSvg.width,
            'height': testSvg.height,
            'baseline': testSvg.baseline,
            if (script.contains('"kind":"mermaid"')) 'png': png,
          },
        }),
      ),
    );
  }
}

class RenderResult {
  RenderResult(Future<VisualSvg> future) {
    future.then(
      (value) {
        this.value = value;
        completed = true;
      },
      onError: (Object error) {
        this.error = error;
        completed = true;
      },
    );
  }
  VisualSvg? value;
  Object? error;
  bool completed = false;
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  final assets = <String>[];
  final previous = WebViewPlatform.instance;
  late VisualPlatform platform;
  setUp(() {
    platform = VisualPlatform();
    WebViewPlatform.instance = platform;
    assets.clear();
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMessageHandler('flutter/assets', (message) {
          final key = utf8.decode(message!.buffer.asUint8List());
          assets.add(key);
          return SynchronousFuture(
            ByteData.sublistView(utf8.encode('/* Bundled fixture: $key */')),
          );
        });
  });
  tearDown(() {
    WebViewPlatform.instance = previous ?? BrowserPlatform();
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMessageHandler('flutter/assets', null);
  });

  group('SVG validation', () {
    test('accepts local paths and rejects external resources', () {
      expect(testSvg.validate, returnsNormally);
      expect(
        () => const VisualSvg(
          source: '<svg><defs><path id="a"/></defs><use href="#a"/></svg>',
          width: 1,
          height: 1,
        ).validate(),
        returnsNormally,
      );
      for (final source in [
        '<svg><script/></svg>',
        '<svg><foreignObject/></svg>',
        '<svg><image href="https://example.com"/></svg>',
        '<svg onclick="x()"/>',
        '<svg><path fill="url(https://example.com)"/></svg>',
        '<svg><style>@import "https://example.com";</style></svg>',
      ]) {
        expect(
          () => VisualSvg(source: source, width: 1, height: 1).validate(),
          throwsFormatException,
          reason: source,
        );
      }
      expect(
        () => VisualSvg(
          source: testSvg.source,
          width: double.infinity,
          height: 1,
        ).validate(),
        throwsFormatException,
      );
      expect(
        () => VisualSvg(
          source: testSvg.source,
          width: 1,
          height: 1,
          baseline: 2,
        ).validate(),
        throwsFormatException,
      );
    });
  });

  group('cache', () {
    test('shares identical requests and captures distinct themes', () async {
      final requests = <VisualRequest>[];
      final renderer = VisualRenderer(
        render: (request) async {
          requests.add(request);
          return testSvg;
        },
      );
      final first = renderer.render(request('x'));
      expect(renderer.render(request('x')), same(first));
      expect((await first).baseline, 8);
      expect(renderer.render(request('x')), same(first));
      await renderer.render(request('x', dark: true));
      expect(requests.map((value) => value.dark), [false, true]);
      renderer.dispose();
      await expectLater(renderer.render(request('fresh')), throwsStateError);
    });

    test('bounds retained bytes and entries', () async {
      var calls = 0;
      var large = true;
      final source =
          '<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><!--${'x' * 1600000}--><path d="M0 0L20 10"/></svg>';
      final renderer = VisualRenderer(
        render: (_) async {
          calls++;
          return large
              ? VisualSvg(source: source, width: 20, height: 10)
              : testSvg;
        },
      );
      for (var index = 0; index < 6; index++) {
        await renderer.render(request('large-$index'));
      }
      expect(calls, 6);
      await renderer.render(request('large-0'));
      expect(calls, 7, reason: 'Completed SVGs exceeding 16 MiB are evicted');
      large = false;
      for (var index = 0; index < 129; index++) {
        await renderer.render(request('small-$index'));
      }
      final before = calls;
      await renderer.render(request('small-0'));
      expect(
        calls,
        before + 1,
        reason: 'Completed cache retains at most 128 entries',
      );
      renderer.dispose();
      final fresh = VisualRenderer(
        render: (_) async {
          calls++;
          return testSvg;
        },
      );
      await fresh.render(request('small-0'));
      expect(calls, before + 2, reason: 'Disposal releases the scoped cache');
      fresh.dispose();
    });
  });

  group('offline renderer', () {
    test('loads bundled assets and serializes jobs', () {
      fakeAsync((clock) {
        platform.hold = true;
        final renderer = VisualRenderer();
        final first = RenderResult(renderer.render(request('x')));
        final second = RenderResult(
          renderer.render(request('graph TD; A-->B', kind: VisualKind.mermaid)),
        );
        clock.flushMicrotasks();
        expect(platform.controllers, hasLength(1));
        final controller = platform.controllers.single;
        expect(controller.calls, hasLength(1));
        expect(assets, [
          for (final name in [
            'config',
            'mermaid',
            'mathjax',
            'fonts',
            'render',
          ])
            'assets/visuals/$name.js',
        ]);
        expect(controller.pages.single, contains("connect-src 'none'"));
        expect(
          controller.pages.single,
          contains(
            'MathJax.loader.paths = {mathjax:"sailry-visual://localhost"}',
          ),
        );
        expect(controller.pages.single.split('</script>'), hasLength(2));
        NavigationDecision? decision;
        Future<NavigationDecision>.value(
          platform.navigation.request(
            const NavigationRequest(
              url: 'https://example.com',
              isMainFrame: true,
            ),
          ),
        ).then((value) => decision = value);
        clock.flushMicrotasks();
        expect(decision, NavigationDecision.prevent);
        controller.reply(controller.calls.single);
        clock.flushMicrotasks();
        expect(first.value?.source, testSvg.source);
        expect(controller.calls, hasLength(2));
        controller.reply(controller.calls.last);
        clock.flushMicrotasks();
        expect(second.value?.png, base64Decode(png));
        expect(second.error, isNull);
        renderer.dispose();
        clock.flushMicrotasks();
        expect(controller.removals, 1);
      });
    });

    test('accepts escaped envelopes within SVG limits', () {
      fakeAsync((clock) {
        platform.output =
            '<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><desc>${'"' * 1200000}</desc><path d="M0 0L20 10"/></svg>';
        final renderer = VisualRenderer();
        final result = RenderResult(renderer.render(request('x')));
        clock.flushMicrotasks();
        expect(result.completed, isTrue);
        expect(result.error, isNull);
        expect(result.value?.source, platform.output);
        renderer.dispose();
        clock.flushMicrotasks();
      });
    });

    test('releases failed loads and permits a fresh source', () {
      fakeAsync((clock) {
        platform.rejectLoad = true;
        final renderer = VisualRenderer();
        final failed = RenderResult(renderer.render(request('first')));
        clock.flushMicrotasks();
        expect(failed.error, isStateError);
        expect(renderer.controller, isNull);
        expect(platform.controllers.single.removals, 1);
        clock.elapse(const Duration(seconds: 31));
        platform.rejectLoad = false;
        final fresh = RenderResult(renderer.render(request('second')));
        clock.flushMicrotasks();
        expect(platform.controllers, hasLength(2));
        expect(fresh.value?.source, testSvg.source);
        renderer.dispose();
        clock.flushMicrotasks();
      });
    });

    test('drops timed out jobs before reinitializing', () {
      fakeAsync((clock) {
        platform.hold = true;
        final renderer = VisualRenderer();
        final active = RenderResult(renderer.render(request('first')));
        final queued = RenderResult(renderer.render(request('queued')));
        clock.flushMicrotasks();
        final failed = platform.controllers.single;
        clock.elapse(const Duration(seconds: 31));
        expect(active.error, isA<TimeoutException>());
        expect(queued.error, isA<TimeoutException>());
        expect(failed.calls, hasLength(1));
        expect(failed.removals, 1);
        expect(renderer.controller, isNull);
        platform.hold = false;
        final fresh = RenderResult(renderer.render(request('fresh')));
        clock.flushMicrotasks();
        expect(platform.controllers, hasLength(2));
        expect(fresh.value?.source, testSvg.source);
        renderer.dispose();
        clock.flushMicrotasks();
      });
    });

    for (final reply in [false, true]) {
      test('bounds held dispatch with reply $reply', () {
        fakeAsync((clock) {
          platform.holdDispatch = true;
          platform.replyBeforeHold = reply;
          final renderer = VisualRenderer();
          final active = RenderResult(renderer.render(request('active')));
          final queued = RenderResult(renderer.render(request('queued')));
          clock.flushMicrotasks();
          final controller = platform.controllers.single;
          clock.elapse(const Duration(seconds: 31));
          if (reply) {
            expect(active.value?.source, testSvg.source);
          } else {
            expect(active.error, isA<TimeoutException>());
          }
          expect(queued.error, isA<TimeoutException>());
          expect(controller.calls, hasLength(1));
          expect(controller.removals, 1);
          expect(renderer.controller, isNull);
          renderer.dispose();
          clock.flushMicrotasks();
        });
      });
    }

    test('ignores late initialization after deadline', () {
      fakeAsync((clock) {
        platform.holdMode = true;
        final renderer = VisualRenderer();
        final first = RenderResult(renderer.render(request('first')));
        clock.flushMicrotasks();
        final expired = platform.controllers.single;
        clock.elapse(const Duration(seconds: 31));
        expect(first.error, isA<TimeoutException>());
        expect(expired.removals, 1);
        expect(renderer.controller, isNull);
        platform.holdMode = false;
        final second = RenderResult(renderer.render(request('second')));
        clock.flushMicrotasks();
        expired.mode.complete();
        clock.flushMicrotasks();
        expect(expired.pages, isEmpty);
        expect(platform.controllers.last.calls, hasLength(1));
        expect(second.value?.source, testSvg.source);
        renderer.dispose();
        clock.flushMicrotasks();
      });
    });

    test('returns to a superseded theme successfully', () {
      fakeAsync((clock) {
        platform.hold = true;
        final renderer = VisualRenderer();
        final active = RenderResult(renderer.render(request('active')));
        clock.flushMicrotasks();
        final old = RenderResult(renderer.render(request('queued')));
        final dark = RenderResult(
          renderer.render(request('queued', dark: true)),
        );
        final current = RenderResult(renderer.render(request('queued')));
        clock.flushMicrotasks();
        expect(old.error, isStateError);
        expect(dark.error, isStateError);
        final controller = platform.controllers.single;
        platform.hold = false;
        controller.reply(controller.calls.single);
        clock.flushMicrotasks();
        expect(active.value?.source, testSvg.source);
        expect(current.value?.source, testSvg.source);
        expect(controller.calls, hasLength(2));
        expect(controller.calls.last, contains('"dark":false'));
        renderer.dispose();
        clock.flushMicrotasks();
      });
    });

    test('observes cancellation before a queued consumer attaches', () {
      fakeAsync((clock) {
        platform.hold = true;
        final renderer = VisualRenderer();
        RenderResult(renderer.render(request('active')));
        clock.flushMicrotasks();
        final obsolete = renderer.render(request('queued'));
        final current = RenderResult(
          renderer.render(request('queued', dark: true)),
        );
        clock.flushMicrotasks();
        final cancelled = RenderResult(obsolete);
        clock.flushMicrotasks();
        expect(cancelled.error, isStateError);
        platform.hold = false;
        final controller = platform.controllers.single;
        controller.reply(controller.calls.single);
        clock.flushMicrotasks();
        expect(current.value?.source, testSvg.source);
        expect(controller.calls, hasLength(2));
        expect(controller.calls.last, contains('"dark":true'));
        renderer.dispose();
        clock.flushMicrotasks();
      });
    });

    test('renders the final captured theme under load', () {
      fakeAsync((clock) {
        platform.hold = true;
        final renderer = VisualRenderer();
        for (var index = 0; index < 80; index++) {
          RenderResult(renderer.render(request('x_$index')));
        }
        clock.flushMicrotasks();
        final current = [
          for (var index = 0; index < 80; index++)
            RenderResult(renderer.render(request('x_$index', dark: true))),
        ];
        platform.hold = false;
        final controller = platform.controllers.single;
        controller.reply(controller.calls.single);
        clock.flushMicrotasks();
        expect(
          current.every((result) => result.value?.source == testSvg.source),
          isTrue,
        );
        expect(controller.calls, hasLength(81));
        expect(
          controller.calls
              .skip(1)
              .every((script) => script.contains('"dark":true')),
          isTrue,
        );
        renderer.dispose();
        clock.flushMicrotasks();
      });
    });

    test('observes abandoned renders when the owner closes', () {
      fakeAsync((clock) {
        platform.hold = true;
        final renderer = VisualRenderer();
        final active = RenderResult(renderer.render(request('active')));
        clock.flushMicrotasks();
        final queued = renderer.render(request('queued'));
        renderer.dispose();
        clock.flushMicrotasks();
        final abandoned = RenderResult(queued);
        clock.flushMicrotasks();
        expect(active.error, isStateError);
        expect(abandoned.error, isStateError);
        expect(platform.controllers.single.removals, 1);
      });
    });
  });
}
