import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_svg/flutter_svg.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/content/code.dart';
import 'package:sailry_mobile/content/markdown.dart';
import 'package:sailry_mobile/content/visuals.dart';
import 'package:sailry_mobile/content/visuals/image.dart';
import 'package:sailry_mobile/ui/theme.dart';
import 'package:webview_flutter/webview_flutter.dart';
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

Finder get formulaPictures => find.byWidgetPredicate(
  (widget) => widget is SvgPicture && widget.bytesLoader is SvgStringLoader,
);

Future<void> mount(
  WidgetTester tester,
  Widget child, {
  VisualRender? render,
  Brightness brightness = Brightness.light,
  Duration animation = Duration.zero,
  bool Function()? ready,
}) async {
  await tester.pumpWidget(
    MaterialApp(
      theme: SailryTheme.of(brightness),
      themeAnimationDuration: animation,
      home: Scaffold(
        body: SingleChildScrollView(
          child: VisualScope(render: render, child: child),
        ),
      ),
    ),
  );
  if (ready != null) {
    for (var attempt = 0; attempt < 500 && !ready(); attempt++) {
      await tester.runAsync(
        () => Future<void>.delayed(const Duration(milliseconds: 10)),
      );
      // Asset replies resume in the widget test's fake zone; flush it separately.
      await tester.pump();
    }
    expect(ready(), isTrue, reason: 'Visual fixture did not initialize');
  }
  await tester.pumpAndSettle();
}

class VisualPlatform extends BrowserPlatform {
  final controllers = <VisualController>[];
  int paints = 0;
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

  @override
  PlatformWebViewWidget createPlatformWebViewWidget(
    PlatformWebViewWidgetCreationParams params,
  ) => VisualWidget(params, this);
}

class VisualWidget extends BrowserWidget {
  VisualWidget(super.params, this.platform);
  final VisualPlatform platform;

  @override
  Widget build(BuildContext context) => CustomPaint(
    painter: SurfacePainter(platform),
    child: const SizedBox.expand(),
  );
}

class SurfacePainter extends CustomPainter {
  SurfacePainter(this.platform);
  final VisualPlatform platform;

  @override
  void paint(Canvas canvas, Size size) => platform.paints++;

  @override
  bool shouldRepaint(SurfacePainter oldDelegate) => false;
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

void main() {
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

  testWidgets('shares results and uses themed inline baselines', (
    tester,
  ) async {
    final requests = <VisualRequest>[];
    Future<VisualSvg> render(VisualRequest request) async {
      requests.add(request);
      return testSvg;
    }

    const content = Column(
      children: [
        MarkdownContent(r'Before $x$ after.'),
        MarkdownContent(r'$x$'),
      ],
    );
    await mount(tester, content, render: render);
    expect(requests, hasLength(1));
    expect(requests.single.kind, VisualKind.inlineMath);
    expect(requests.single.dark, isFalse);
    expect(formulaPictures, findsNWidgets(2));
    expect(
      tester
          .widgetList<FormulaBaseline>(find.byType(FormulaBaseline))
          .every((value) => value.baseline == 8),
      isTrue,
    );
    expect(find.byType(WebViewWidget, skipOffstage: false), findsNothing);
    for (final picture in tester.widgetList<SvgPicture>(formulaPictures)) {
      final loader = picture.bytesLoader as SvgStringLoader;
      await tester.runAsync(() => loader.loadBytes(null));
      expect(svg.cache.evict(loader.cacheKey(null)), isFalse);
    }
    await mount(tester, content, render: render, brightness: Brightness.dark);
    expect(requests, hasLength(2));
    expect(requests.last.dark, isTrue);
    expect(requests.last.foreground, isNot(requests.first.foreground));
    expect(tester.takeException(), isNull);
  });

  testWidgets('tall inline formulas fit their paragraph', (tester) async {
    const tall = VisualSvg(
      source:
          '<svg xmlns="http://www.w3.org/2000/svg" width="203.64" height="29.51"><path d="M0 0L203.64 29.51"/></svg>',
      width: 203.64,
      height: 29.51,
      baseline: 18.88,
    );
    final pending = Completer<VisualSvg>();
    await mount(
      tester,
      const SizedBox(
        width: 320,
        child: MarkdownContent(r'Before $fraction$ after text.'),
      ),
      render: (_) => pending.future,
    );
    expect(formulaPictures, findsNothing);
    pending.complete(tall);
    await tester.pumpAndSettle();
    final picture = tester.getRect(formulaPictures);
    final paragraph = find.byWidgetPredicate(
      (widget) =>
          widget is SelectableText &&
          widget.textSpan?.toPlainText().contains('Before') == true,
    );
    final bounds = tester.getRect(paragraph);
    expect(picture.top, greaterThanOrEqualTo(bounds.top));
    expect(picture.bottom, lessThanOrEqualTo(bounds.bottom));
    expect(bounds.height, greaterThanOrEqualTo(picture.height));
    expect(
      tester.widget<SelectableText>(paragraph).textSpan!.toPlainText(),
      contains('after text.'),
    );
    expect(tester.takeException(), isNull);
  });

  testWidgets('plain paragraphs retain fixed line height', (tester) async {
    await mount(tester, const MarkdownContent('Plain text paragraph.'));
    final paragraph = tester.widget<SelectableText>(
      find.byWidgetPredicate(
        (widget) =>
            widget is SelectableText &&
            widget.textSpan?.toPlainText() == 'Plain text paragraph.',
      ),
    );
    expect(paragraph.strutStyle!.forceStrutHeight, isTrue);
    expect(paragraph.strutStyle!.height, 1.7);
    expect(paragraph.strutStyle!.fontSize, 14);
    expect(tester.takeException(), isNull);
  });

  testWidgets('preserves failed and unfinished source', (tester) async {
    const content = MarkdownContent(
      r'Inline $x$ and unfinished \(value'
      '\n\n```mermaid\ngraph INVALID\n```',
    );
    await mount(
      tester,
      content,
      render: (_) async => throw const FormatException('Invalid fixture'),
    );
    expect(formulaPictures, findsNothing);
    expect(find.textContaining(r'$x$', findRichText: true), findsWidgets);
    expect(find.textContaining('(value', findRichText: true), findsWidgets);
    expect(
      tester.widget<CodeBlock>(find.byType(CodeBlock)).text,
      'graph INVALID\n',
    );
    expect(tester.takeException(), isNull);
  });

  testWidgets('bounds retained SVG bytes and entries', (tester) async {
    var calls = 0;
    var large = true;
    final source =
        '<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><!--${'x' * 1600000}--><path d="M0 0L20 10"/></svg>';
    Future<VisualSvg> render(VisualRequest _) async {
      calls++;
      return large ? VisualSvg(source: source, width: 20, height: 10) : testSvg;
    }

    Widget content(String value) =>
        MarkdownVisual(kind: VisualKind.displayMath, source: value);
    for (var index = 0; index < 6; index++) {
      await mount(tester, content('large-$index'), render: render);
    }
    expect(calls, 6);
    await mount(tester, content('large-0'), render: render);
    expect(calls, 7, reason: 'Completed SVGs exceeding 16 MiB are evicted');
    large = false;
    for (var index = 0; index < 129; index++) {
      await mount(tester, content('small-$index'), render: render);
    }
    final before = calls;
    await mount(tester, content('small-0'), render: render);
    expect(
      calls,
      before + 1,
      reason: 'Completed cache retains at most 128 entries',
    );
    await tester.pumpWidget(const SizedBox());
    await mount(tester, content('small-0'), render: render);
    expect(calls, before + 2, reason: 'Disposal releases the scoped cache');
    expect(tester.takeException(), isNull);
  });

  testWidgets('uses one offline surface and serializes jobs', (tester) async {
    final platform = VisualPlatform()..hold = true;
    final previous = WebViewPlatform.instance;
    WebViewPlatform.instance = platform;
    addTearDown(() => WebViewPlatform.instance = previous ?? BrowserPlatform());
    await mount(
      tester,
      const Column(
        children: [
          MarkdownVisual(kind: VisualKind.inlineMath, source: 'x'),
          MarkdownVisual(kind: VisualKind.mermaid, source: 'graph TD; A-->B'),
        ],
      ),
      ready: () =>
          platform.controllers.isNotEmpty &&
          platform.controllers.single.calls.isNotEmpty,
    );
    expect(platform.controllers, hasLength(1));
    final controller = platform.controllers.single;
    expect(controller.calls, hasLength(1));
    expect(controller.pages.single, contains("connect-src 'none'"));
    expect(controller.pages.single, contains('window.sailryRender'));
    expect(
      controller.pages.single,
      contains('MathJax.loader.paths = {mathjax:"sailry-visual://localhost"}'),
    );
    expect(
      controller.pages.single.indexOf('MathJax.loader.paths ='),
      lessThan(controller.pages.single.indexOf('new URL(t,location)')),
    );
    expect(controller.pages.single.split('</script>'), hasLength(2));
    expect(find.byType(WebViewWidget, skipOffstage: false), findsOneWidget);
    expect(
      await platform.navigation.request(
        const NavigationRequest(url: 'https://example.com', isMainFrame: true),
      ),
      NavigationDecision.prevent,
    );
    controller.reply(controller.calls.single);
    await tester.pumpAndSettle();
    expect(controller.calls, hasLength(2));
    controller.reply(controller.calls.last);
    await tester.pumpAndSettle();
    expect(formulaPictures, findsOneWidget);
    expect(find.byType(Image), findsOneWidget);
    await tester.pumpWidget(const SizedBox());
    expect(controller.removals, 1);
    expect(find.byType(WebViewWidget, skipOffstage: false), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('composes a transparent noninteractive native surface', (
    tester,
  ) async {
    final platform = VisualPlatform()..hold = true;
    final previous = WebViewPlatform.instance;
    WebViewPlatform.instance = platform;
    addTearDown(() => WebViewPlatform.instance = previous ?? BrowserPlatform());
    await mount(
      tester,
      const MarkdownVisual(kind: VisualKind.inlineMath, source: 'x'),
      ready: () =>
          platform.controllers.isNotEmpty &&
          platform.controllers.single.calls.isNotEmpty,
    );
    final surface = find.byType(WebViewWidget);
    expect(surface, findsOneWidget);
    expect(platform.paints, greaterThan(0));
    expect(tester.getSize(surface), const Size(640, 480));
    expect(platform.controllers.single.background, Colors.transparent);
    expect(
      tester
          .getRect(surface)
          .overlaps(tester.getRect(find.byType(VisualScope))),
      isTrue,
    );
    expect(
      platform.controllers.single.pages.single,
      contains(
        'html,body{width:640px;min-height:480px;margin:0;background:transparent!important}body{opacity:0}',
      ),
    );
    expect(
      tester
          .widget<IgnorePointer>(
            find
                .ancestor(of: surface, matching: find.byType(IgnorePointer))
                .first,
          )
          .ignoring,
      isTrue,
    );
    expect(
      tester
          .widget<ExcludeSemantics>(
            find
                .ancestor(of: surface, matching: find.byType(ExcludeSemantics))
                .first,
          )
          .excluding,
      isTrue,
    );
    final scope = find.byType(VisualScope);
    expect(
      tester
          .widget<Stack>(
            find.descendant(of: scope, matching: find.byType(Stack)).first,
          )
          .clipBehavior,
      Clip.hardEdge,
    );
    await tester.pumpWidget(const SizedBox());
    await tester.pumpAndSettle();
    expect(tester.takeException(), isNull);
  });

  testWidgets('accepts escaped envelopes within SVG size limits', (
    tester,
  ) async {
    final platform = VisualPlatform()
      ..output =
          '<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><desc>${'"' * 1200000}</desc><path d="M0 0L20 10"/></svg>';
    final previous = WebViewPlatform.instance;
    WebViewPlatform.instance = platform;
    addTearDown(() => WebViewPlatform.instance = previous ?? BrowserPlatform());
    await mount(
      tester,
      const MarkdownVisual(kind: VisualKind.displayMath, source: 'x'),
      ready: () =>
          platform.controllers.isNotEmpty &&
          platform.controllers.single.calls.isNotEmpty,
    );
    expect(formulaPictures, findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('releases failed loads and permits a fresh source', (
    tester,
  ) async {
    final platform = VisualPlatform()..rejectLoad = true;
    final previous = WebViewPlatform.instance;
    WebViewPlatform.instance = platform;
    addTearDown(() => WebViewPlatform.instance = previous ?? BrowserPlatform());
    await mount(
      tester,
      const MarkdownVisual(kind: VisualKind.displayMath, source: 'first'),
      ready: () =>
          platform.controllers.isNotEmpty &&
          platform.controllers.single.removals == 1,
    );
    expect(find.byType(WebViewWidget, skipOffstage: false), findsNothing);
    expect(find.byType(CodeBlock), findsOneWidget);
    expect(platform.controllers.single.removals, 1);
    await tester.pump(const Duration(seconds: 31));
    expect(tester.takeException(), isNull);
    platform.rejectLoad = false;
    await mount(
      tester,
      const MarkdownVisual(kind: VisualKind.displayMath, source: 'second'),
      ready: () =>
          platform.controllers.length == 2 &&
          platform.controllers.last.calls.isNotEmpty,
    );
    expect(platform.controllers, hasLength(2));
    expect(formulaPictures, findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('drops timed out jobs before reinitializing', (tester) async {
    final platform = VisualPlatform()..hold = true;
    final previous = WebViewPlatform.instance;
    WebViewPlatform.instance = platform;
    addTearDown(() => WebViewPlatform.instance = previous ?? BrowserPlatform());
    await mount(
      tester,
      const Column(
        children: [
          MarkdownVisual(kind: VisualKind.displayMath, source: 'first'),
          MarkdownVisual(kind: VisualKind.displayMath, source: 'queued'),
        ],
      ),
      ready: () =>
          platform.controllers.isNotEmpty &&
          platform.controllers.single.calls.isNotEmpty,
    );
    final failed = platform.controllers.single;
    await tester.pump(const Duration(seconds: 31));
    await tester.pumpAndSettle();
    expect(failed.calls, hasLength(1));
    expect(failed.removals, 1);
    expect(find.byType(WebViewWidget, skipOffstage: false), findsNothing);
    platform.hold = false;
    await mount(
      tester,
      const MarkdownVisual(kind: VisualKind.displayMath, source: 'fresh'),
      ready: () =>
          platform.controllers.length == 2 &&
          platform.controllers.last.calls.isNotEmpty,
    );
    expect(platform.controllers, hasLength(2));
    expect(formulaPictures, findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  for (final reply in [false, true]) {
    testWidgets('bounds held dispatch with reply $reply', (tester) async {
      final platform = VisualPlatform()
        ..holdDispatch = true
        ..replyBeforeHold = reply;
      final previous = WebViewPlatform.instance;
      WebViewPlatform.instance = platform;
      addTearDown(
        () => WebViewPlatform.instance = previous ?? BrowserPlatform(),
      );
      await mount(
        tester,
        const Column(
          children: [
            MarkdownVisual(kind: VisualKind.inlineMath, source: 'active'),
            MarkdownVisual(kind: VisualKind.inlineMath, source: 'queued'),
          ],
        ),
        ready: () =>
            platform.controllers.isNotEmpty &&
            platform.controllers.single.calls.isNotEmpty,
      );
      final controller = platform.controllers.single;
      await tester.pump(const Duration(seconds: 31));
      await tester.pumpAndSettle();
      expect(controller.calls, hasLength(1));
      expect(controller.removals, 1);
      expect(find.byType(WebViewWidget, skipOffstage: false), findsNothing);
      await tester.pumpWidget(const SizedBox());
      expect(tester.takeException(), isNull);
    });
  }

  testWidgets('ignores late initialization after deadline', (tester) async {
    final platform = VisualPlatform()..holdMode = true;
    final previous = WebViewPlatform.instance;
    WebViewPlatform.instance = platform;
    addTearDown(() => WebViewPlatform.instance = previous ?? BrowserPlatform());
    await mount(
      tester,
      const MarkdownVisual(kind: VisualKind.displayMath, source: 'first'),
      ready: () => platform.controllers.isNotEmpty,
    );
    final expired = platform.controllers.single;
    await tester.pump(const Duration(seconds: 31));
    await tester.pumpAndSettle();
    expect(expired.removals, 1);
    expect(find.byType(WebViewWidget, skipOffstage: false), findsNothing);
    platform.holdMode = false;
    await mount(
      tester,
      const MarkdownVisual(kind: VisualKind.displayMath, source: 'second'),
      ready: () =>
          platform.controllers.length == 2 &&
          platform.controllers.last.calls.isNotEmpty,
    );
    expired.mode.complete();
    await tester.pumpAndSettle();
    expect(expired.pages, isEmpty);
    expect(platform.controllers.last.calls, hasLength(1));
    expect(formulaPictures, findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('returns to a superseded theme successfully', (tester) async {
    final platform = VisualPlatform()..hold = true;
    final previous = WebViewPlatform.instance;
    WebViewPlatform.instance = platform;
    addTearDown(() => WebViewPlatform.instance = previous ?? BrowserPlatform());
    const content = Column(
      children: [
        MarkdownVisual(kind: VisualKind.inlineMath, source: 'active'),
        MarkdownVisual(kind: VisualKind.inlineMath, source: 'queued'),
      ],
    );
    await mount(
      tester,
      content,
      ready: () =>
          platform.controllers.isNotEmpty &&
          platform.controllers.single.calls.isNotEmpty,
    );
    await mount(tester, content, brightness: Brightness.dark);
    await mount(tester, content, brightness: Brightness.light);
    final controller = platform.controllers.single;
    platform.hold = false;
    controller.reply(controller.calls.single);
    await tester.pumpAndSettle();
    expect(formulaPictures, findsNWidgets(2));
    expect(controller.calls, hasLength(2));
    expect(controller.calls.last, contains('"dark":false'));
    expect(tester.takeException(), isNull);
  });

  testWidgets('observes cancellation before a queued consumer attaches', (
    tester,
  ) async {
    final platform = VisualPlatform()..hold = true;
    final previous = WebViewPlatform.instance;
    WebViewPlatform.instance = platform;
    addTearDown(() => WebViewPlatform.instance = previous ?? BrowserPlatform());
    await mount(
      tester,
      const MarkdownVisual(kind: VisualKind.inlineMath, source: 'active'),
      ready: () =>
          platform.controllers.isNotEmpty &&
          platform.controllers.single.calls.isNotEmpty,
    );
    final dynamic owner = tester.state(find.byType(VisualScope));
    Future<VisualSvg> queued(bool dark) =>
        owner.render(
              VisualRequest(
                kind: VisualKind.inlineMath,
                source: 'queued',
                dark: dark,
                foreground: '#ffffff',
                background: dark ? '#000000' : '#ffffff',
                accent: '#123456',
                fontSize: 14,
              ),
            )
            as Future<VisualSvg>;
    final obsolete = queued(false);
    final current = queued(true);
    await tester.pump();
    expect(tester.takeException(), isNull);
    await expectLater(obsolete, throwsStateError);
    platform.hold = false;
    final controller = platform.controllers.single;
    controller.reply(controller.calls.single);
    await tester.pumpAndSettle();
    expect((await current).source, testSvg.source);
    expect(controller.calls, hasLength(2));
    expect(controller.calls.last, contains('"dark":true'));
    expect(tester.takeException(), isNull);
  });

  testWidgets('renders the final animated theme under load', (tester) async {
    final platform = VisualPlatform()..hold = true;
    final previous = WebViewPlatform.instance;
    WebViewPlatform.instance = platform;
    addTearDown(() => WebViewPlatform.instance = previous ?? BrowserPlatform());
    final content = Column(
      children: [
        for (var index = 0; index < 80; index++)
          MarkdownVisual(kind: VisualKind.inlineMath, source: 'x_$index'),
      ],
    );
    await mount(
      tester,
      content,
      ready: () =>
          platform.controllers.isNotEmpty &&
          platform.controllers.single.calls.isNotEmpty,
    );
    await mount(
      tester,
      content,
      brightness: Brightness.dark,
      animation: const Duration(milliseconds: 200),
    );
    final controller = platform.controllers.single;
    platform.hold = false;
    controller.reply(controller.calls.single);
    await tester.pumpAndSettle();
    expect(formulaPictures, findsNWidgets(80));
    expect(controller.calls, hasLength(81));
    expect(
      controller.calls
          .skip(1)
          .every((script) => script.contains('"dark":true')),
      isTrue,
    );
    expect(tester.takeException(), isNull);
  });

  testWidgets('observes abandoned renders when the scope closes', (
    tester,
  ) async {
    final platform = VisualPlatform()..hold = true;
    final previous = WebViewPlatform.instance;
    WebViewPlatform.instance = platform;
    addTearDown(() => WebViewPlatform.instance = previous ?? BrowserPlatform());
    await mount(
      tester,
      const MarkdownVisual(kind: VisualKind.inlineMath, source: 'active'),
      ready: () =>
          platform.controllers.isNotEmpty &&
          platform.controllers.single.calls.isNotEmpty,
    );
    final dynamic owner = tester.state(find.byType(VisualScope));
    final queued =
        owner.render(
              const VisualRequest(
                kind: VisualKind.inlineMath,
                source: 'queued',
                dark: false,
                foreground: '#000000',
                background: '#ffffff',
                accent: '#123456',
                fontSize: 14,
              ),
            )
            as Future<VisualSvg>;
    await tester.pumpWidget(const SizedBox());
    await tester.pump();
    expect(tester.takeException(), isNull);
    await expectLater(queued, throwsStateError);
    expect(platform.controllers.single.removals, 1);
    expect(tester.takeException(), isNull);
  });
}
