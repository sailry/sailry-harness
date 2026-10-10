import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_svg/flutter_svg.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:sailry_mobile/content/code.dart';
import 'package:sailry_mobile/content/markdown.dart';
import 'package:sailry_mobile/content/visuals.dart';
import 'package:sailry_mobile/ui/theme.dart';
import 'package:webview_flutter/webview_flutter.dart';

const density =
    r'f_X(x)=\frac{1}{\sigma\sqrt{2\pi}}\exp\left(-\frac{(x-\mu)^2}{2\sigma^2}\right)';
const bayes = r'P(A\mid B)=\frac{P(B\mid A)P(A)}{P(B)}';
final source =
    'Normal \\($density\\) and \$x^2\$.\n\n\$\$\ny^2\n\$\$\n\n\\[\n$bayes\n\\]\n\n'
    '```mermaid\ngraph TD\n A[Start] --> B[Finish]\n classDef node fill:#ffdd88,stroke:#aa5500,color:#332200\n class A,B node\n```';

Finder get formulaPictures => find.byWidgetPredicate(
  (widget) => widget is SvgPicture && widget.bytesLoader is SvgStringLoader,
);

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();
  testWidgets('renders offline formulas and styled diagram natively', (
    tester,
  ) async {
    for (final brightness in Brightness.values) {
      await tester.pumpWidget(
        MaterialApp(
          theme: SailryTheme.of(brightness),
          home: Scaffold(
            body: Align(
              alignment: Alignment.topLeft,
              child: SizedBox(
                width: 320,
                child: SingleChildScrollView(
                  child: VisualScope(
                    key: ValueKey(brightness),
                    child: MarkdownContent(source),
                  ),
                ),
              ),
            ),
          ),
        ),
      );
      await until(
        tester,
        () =>
            formulaPictures.evaluate().length == 4 &&
            find.byType(Image).evaluate().length == 1,
        phase: brightness.name,
      );
      expect(find.byType(WebViewWidget, skipOffstage: false), findsOneWidget);
      expect(find.byType(CodeBlock), findsNothing);
      final surface = tester.widget<WebViewWidget>(find.byType(WebViewWidget));
      final controller = WebViewController.fromPlatform(
        surface.platform.params.controller,
      );
      final boot =
          jsonDecode(
                (await controller.runJavaScriptReturningResult(
                  '''JSON.stringify({
width:document.body.clientWidth,height:document.body.clientHeight,
viewport:[innerWidth,innerHeight],
scripts:document.scripts.length,hidden:getComputedStyle(document.body).opacity==='0',
fonts:MathJax._.components.package.Package.packages.get('[mathjax-tex]/svg').hasFailed
})''',
                )).toString(),
              )
              as Map<String, dynamic>;
      expect(boot['width'], 640);
      expect(boot['height'], greaterThanOrEqualTo(480));
      expect(boot['viewport'][0], greaterThan(0));
      expect(boot['viewport'][1], greaterThan(0));
      expect(boot['scripts'], 1);
      expect(boot['hidden'], isTrue);
      expect(boot['fonts'], isFalse);
      final formulas = tester.widgetList<SvgPicture>(formulaPictures);
      for (final formula in formulas) {
        final svg = (formula.bytesLoader as SvgStringLoader).provideSvg(null);
        expect(svg, contains('<path'));
        expect(svg, isNot(contains('currentColor')));
        final bytes = await tester.runAsync(
          () => formula.bytesLoader.loadBytes(null),
        );
        expect(bytes!.lengthInBytes, greaterThan(0));
      }
      final image = tester.widget<Image>(find.byType(Image));
      expect((image.image as MemoryImage).bytes.length, greaterThan(200));
      final visual = find.byWidgetPredicate(
        (widget) =>
            widget is MarkdownVisual && widget.kind == VisualKind.mermaid,
      );
      final job = tester
          .widget<FutureBuilder<VisualSvg>>(
            find.descendant(
              of: visual,
              matching: find.byType(FutureBuilder<VisualSvg>),
            ),
          )
          .future!;
      final diagram = await job;
      expect(diagram.source, contains('<style'));
      expect(diagram.source, contains('#ffdd88'));
      expect(diagram.source, contains('marker-end'));
      expect(diagram.png, orderedEquals((image.image as MemoryImage).bytes));
      final decoded = Completer<ImageInfo>();
      final stream = image.image.resolve(ImageConfiguration.empty);
      final listener = ImageStreamListener(
        (info, _) => decoded.complete(info),
        onError: (Object error, StackTrace? stack) =>
            decoded.completeError(error, stack),
      );
      stream.addListener(listener);
      final info = await tester.runAsync(
        () => decoded.future.timeout(const Duration(seconds: 5)),
      );
      stream.removeListener(listener);
      expect(info!.image.width, greaterThan(20));
      expect(info.image.height, greaterThan(20));
      expect(tester.getSize(find.byType(Image)).width, lessThanOrEqualTo(320));
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox());
      await tester.pump();
      expect(find.byType(WebViewWidget, skipOffstage: false), findsNothing);
    }
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: VisualScope(
            child: const MarkdownContent(
              '```mermaid\ninvalid diagram source\n```',
            ),
          ),
        ),
      ),
    );
    await until(
      tester,
      () => find
          .byKey(const ValueKey('visual-source-done'))
          .evaluate()
          .isNotEmpty,
    );
    expect(find.byType(Image), findsNothing);
    expect(formulaPictures, findsNothing);
    expect(
      tester.widget<CodeBlock>(find.byType(CodeBlock)).text,
      'invalid diagram source\n',
    );
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox());
    await tester.pump();
    expect(find.byType(WebViewWidget, skipOffstage: false), findsNothing);
  });
}

Future<void> until(
  WidgetTester tester,
  bool Function() ready, {
  String phase = 'fallback',
}) async {
  final deadline = DateTime.now().add(const Duration(seconds: 45));
  final probe = NativeVisualProbe(tester, phase);
  var inspected = false;
  while (!ready()) {
    probe.observe();
    if (!inspected &&
        DateTime.now().isAfter(
          deadline.subtract(const Duration(seconds: 35)),
        )) {
      inspected = true;
      await probe.report();
    }
    if (DateTime.now().isAfter(deadline)) {
      await probe.report();
      fail('Native visual rendering timed out ($phase)');
    }
    await tester.pump(const Duration(milliseconds: 100));
    await Future<void>.delayed(const Duration(milliseconds: 100));
  }
  await tester.pump();
}

class NativeVisualProbe {
  NativeVisualProbe(this.tester, this.phase);
  final WidgetTester tester;
  final String phase;
  final outcomes = <Future<VisualSvg>, String>{};

  void observe() {
    for (final element in find.byType(MarkdownVisual).evaluate()) {
      final visual = element.widget as MarkdownVisual;
      final label = '${visual.kind.name}: ${visual.source}';
      final builders = find.descendant(
        of: find.byWidget(visual),
        matching: find.byType(FutureBuilder<VisualSvg>),
      );
      for (final builder in tester.widgetList<FutureBuilder<VisualSvg>>(
        builders,
      )) {
        final future = builder.future;
        if (future == null || outcomes.containsKey(future)) continue;
        outcomes[future] = '$label pending';
        unawaited(
          future.then<void>(
            (svg) => outcomes[future] =
                '$label completed ${svg.width}x${svg.height} '
                'SVG ${svg.source.length} PNG ${svg.png?.length ?? 0}',
            onError: (Object error, StackTrace stack) {
              outcomes[future] = '$label failed: $error\n$stack';
            },
          ),
        );
      }
    }
  }

  Future<void> report() async {
    observe();
    final surfaces = find.byType(WebViewWidget, skipOffstage: false);
    debugPrint(
      'Native visuals ($phase): formulas ${formulaPictures.evaluate().length}, '
      'images ${find.byType(Image).evaluate().length}, '
      'source blocks ${find.byType(CodeBlock).evaluate().length}, '
      'WebViews ${surfaces.evaluate().length}\n${outcomes.values.join('\n')}',
    );
    for (final widget in tester.widgetList<WebViewWidget>(surfaces)) {
      final controller = WebViewController.fromPlatform(
        widget.platform.params.controller,
      );
      try {
        final state = await controller
            .runJavaScriptReturningResult('''JSON.stringify({
url:location.href, ready:document.readyState, scripts:document.scripts.length,
viewport:[innerWidth,innerHeight], body:[document.body?.clientWidth,document.body?.clientHeight],
surface:!!document.getElementById('surface'), mermaid:typeof window.mermaid,
mathjax:typeof window.MathJax?.tex2svgPromise,
mathjaxReady:!!window.MathJax?.startup?.document,
render:typeof window.sailryRender, raster:typeof window.sailryVisual,
channel:typeof window.SailryVisual?.postMessage,
font:typeof window.MathJax?.config?.svg?.fontData,
packages:[...(window.MathJax?._?.components?.package?.Package?.packages||[])].map(([name,p])=>({name,loaded:p.isLoaded,loading:p.isLoading,failed:p.hasFailed,dependencies:p.dependencyCount})),
scripts:[...document.scripts].map(script=>script.src),
startup:window.MathJax?.config?.startup
})''')
            .timeout(const Duration(seconds: 2));
        debugPrint('Native visual page ($phase): $state');
      } catch (error, stack) {
        debugPrint('Native visual page ($phase) failed: $error\n$stack');
      }
    }
  }
}
