import 'dart:async';
import 'dart:convert';
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:flutter_svg/flutter_svg.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:sailry_mobile/content/code.dart';
import 'package:sailry_mobile/content/markdown.dart';
import 'package:sailry_mobile/content/visuals.dart';
import 'package:sailry_mobile/ui/theme.dart';
import 'package:webview_flutter/webview_flutter.dart';

import 'support/webview.dart';

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
  testWidgets('renders offline formulas and monochrome diagram natively', (
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
      final boot = await readObject(controller, '''({
width:document.body.clientWidth,height:document.body.clientHeight,
viewport:[innerWidth,innerHeight],pixel_ratio:devicePixelRatio,
scripts:document.scripts.length,hidden:getComputedStyle(document.body).opacity==='0',
fonts:MathJax._.components.package.Package.packages.get('[mathjax-tex]/svg').hasFailed
})''');
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
      final densityVisual = find.byWidgetPredicate(
        (widget) => widget is MarkdownVisual && widget.source == density,
      );
      final densityJob = tester
          .widget<FutureBuilder<VisualSvg>>(
            find.descendant(
              of: densityVisual,
              matching: find.byType(FutureBuilder<VisualSvg>),
            ),
          )
          .future!;
      final densitySvg = await densityJob;
      expect(
        RegExp('data-mml-node="mfrac"').allMatches(densitySvg.source).length,
        2,
      );
      expect(densitySvg.source, contains('data-mml-node="msqrt"'));
      final paragraph = find.byWidgetPredicate(
        (widget) =>
            widget is SelectableText &&
            widget.textSpan?.toPlainText().startsWith('Normal ') == true,
      );
      final densityPicture = find.descendant(
        of: densityVisual,
        matching: formulaPictures,
      );
      final paragraphBounds = tester.getRect(paragraph);
      final densityBounds = tester.getRect(densityPicture);
      expect(densityBounds.top, greaterThanOrEqualTo(paragraphBounds.top));
      expect(densityBounds.bottom, lessThanOrEqualTo(paragraphBounds.bottom));

      await controller.runJavaScript(
        'window.formulaProbe={};MathJax.tex2svgPromise(${jsonEncode(density)},{display:false}).then(node=>window.formulaProbe={count:node.querySelectorAll("svg").length,fractions:node.querySelectorAll("[data-mml-node=mfrac]").length},error=>window.formulaProbe={error:String(error)})',
      );
      Map<String, dynamic> formulaProbe = {};
      for (var attempt = 0; attempt < 100; attempt++) {
        formulaProbe = await readObject(controller, 'window.formulaProbe');
        if (formulaProbe.isNotEmpty) break;
        await tester.pump(const Duration(milliseconds: 100));
        await Future<void>.delayed(const Duration(milliseconds: 100));
      }
      expect(formulaProbe['count'], 1);
      expect(formulaProbe['fractions'], 2);

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
      expect(tester.widget<MarkdownVisual>(visual).source, contains('#ffdd88'));
      expect(diagram.source, contains('marker-end'));
      expect(diagram.png, orderedEquals((image.image as MemoryImage).bytes));
      final foreground = SailryTheme.of(brightness).colorScheme.onSurface;
      final color =
          '#${foreground.toARGB32().toRadixString(16).padLeft(8, '0').substring(2)}';
      final paint = await readObject(controller, '''(() => {
const wrapper=document.createElement('div');wrapper.style.color=${jsonEncode(color)};
wrapper.innerHTML=${jsonEncode(diagram.source)};document.body.append(wrapper);
try {
 const svg=wrapper.querySelector('svg'), expected=getComputedStyle(wrapper).color;
 const nodes=[...svg.querySelectorAll('.node > rect')].map(node=>({fill:getComputedStyle(node).fill,stroke:getComputedStyle(node).stroke}));
 return {expected,nodes,filtered:[svg,...svg.querySelectorAll('*')].some(node=>getComputedStyle(node).filter!=='none'),labels:[...svg.querySelectorAll('text,tspan')].map(node=>getComputedStyle(node).fill),line:getComputedStyle(svg.querySelector('.flowchart-link')).stroke,marker:getComputedStyle(svg.querySelector('marker path')).fill};
} finally {wrapper.remove();}
})()''');
      expect(paint['nodes'], hasLength(2));
      expect(paint['filtered'], isFalse);
      for (final node in (paint['nodes'] as List).cast<Map>()) {
        expect(node['fill'], 'none');
        expect(node['stroke'], paint['expected']);
      }
      expect(paint['labels'], isNotEmpty);
      expect(
        (paint['labels'] as List).every((fill) => fill == paint['expected']),
        isTrue,
      );
      expect(paint['line'], paint['expected']);
      expect(paint['marker'], paint['expected']);

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
      final ratio = (boot['pixel_ratio'] as num).toDouble();
      expect(ratio, greaterThanOrEqualTo(1));
      expect(info.image.width, (diagram.width * ratio).floor());
      expect(info.image.height, (diagram.height * ratio).floor());
      expect(info.image.width * info.image.height, lessThanOrEqualTo(2097152));

      final pixels = await tester.runAsync(
        () => info.image.toByteData(format: ui.ImageByteFormat.rawRgba),
      );
      expect(pixels, isNotNull);
      expect(
        pixels!.getUint8(3),
        0,
        reason: 'Diagram canvas keeps its transparent background',
      );

      if (brightness == Brightness.light) {
        // Exercise the real canvas adapter with a large but valid SVG.
        await controller.runJavaScript(r'''window.rasterProbe={};(async()=>{
const render=window.sailryRender;
try {
 window.sailryRender=async()=>({source:'<svg xmlns="http://www.w3.org/2000/svg" width="4096" height="1024"><path d="M0 0L4096 1024" stroke="black"/></svg>',width:4096,height:1024});
 const result=await window.sailryVisual({kind:'mermaid'});
 const bytes=Uint8Array.from(atob(result.png),value=>value.charCodeAt(0));
 const data=new DataView(bytes.buffer);
 window.rasterProbe={width:data.getUint32(16),height:data.getUint32(20)};
} catch(error) {window.rasterProbe={error:String(error)};}
finally {window.sailryRender=render;}
})()''');
        Map<String, dynamic> rasterProbe = {};
        for (var attempt = 0; attempt < 100; attempt++) {
          rasterProbe = await readObject(controller, 'window.rasterProbe');
          if (rasterProbe.isNotEmpty) break;
          await tester.pump(const Duration(milliseconds: 100));
          await Future<void>.delayed(const Duration(milliseconds: 100));
        }
        expect(rasterProbe['error'], isNull);
        expect(rasterProbe['width'], greaterThan(0));
        expect(rasterProbe['height'], greaterThan(0));
        expect(rasterProbe['width'], lessThanOrEqualTo(4096));
        expect(rasterProbe['height'], lessThanOrEqualTo(4096));
        expect(
          (rasterProbe['width'] as int) * (rasterProbe['height'] as int),
          lessThanOrEqualTo(2097152),
        );
      }

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
viewport:[innerWidth,innerHeight],pixel_ratio:devicePixelRatio, body:[document.body?.clientWidth,document.body?.clientHeight],
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
