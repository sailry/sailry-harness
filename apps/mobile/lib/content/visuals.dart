import 'dart:math' as math;

import 'package:flutter/material.dart';
import 'package:flutter_svg/flutter_svg.dart';
import 'package:webview_flutter/webview_flutter.dart';

import '../l10n/strings.dart';
import '../ui/kit.dart';
import 'code.dart';
import 'visuals/data.dart';
import 'visuals/image.dart';
import 'visuals/renderer.dart';

export 'visuals/data.dart';
export 'visuals/renderer.dart' show VisualRender;

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

class _VisualScopeState extends State<VisualScope> {
  late final _renderer = VisualRenderer(render: widget.render);

  @override
  void initState() {
    super.initState();
    _renderer.addListener(_changed);
  }

  @override
  void didUpdateWidget(VisualScope oldWidget) {
    super.didUpdateWidget(oldWidget);
    _renderer.renderOverride = widget.render;
  }

  void _changed() {
    if (mounted) setState(() {});
  }

  @override
  void dispose() {
    _renderer.removeListener(_changed);
    _renderer.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => _VisualHost(
    owner: _renderer,
    child: Stack(
      children: [
        widget.child,
        if (_renderer.controller != null)
          // UIKit sizes the native view during composition. Keep it composed;
          // the transparent page hides its DOM while preserving SVG layout.
          Positioned(
            left: 0,
            top: 0,
            width: 640,
            height: 480,
            child: IgnorePointer(
              child: ExcludeSemantics(
                child: WebViewWidget(controller: _renderer.controller!),
              ),
            ),
          ),
      ],
    ),
  );
}

class _VisualHost extends InheritedWidget {
  const _VisualHost({required this.owner, required super.child});
  final VisualRenderer owner;
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
                child: FormulaBaseline(
                  baseline: (svg.baseline ?? svg.height) * scale,
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
