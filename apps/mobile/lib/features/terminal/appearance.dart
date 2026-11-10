import 'dart:math' as math;
import 'package:flutter/material.dart';

double terminalFontSize(BuildContext context) => MediaQuery.textScalerOf(
  context,
).scale(Theme.of(context).textTheme.bodyMedium?.fontSize ?? 14);

Size terminalCell(BuildContext context) {
  final fontSize = terminalFontSize(context);
  final painter = TextPainter(
    text: TextSpan(
      text: 'M',
      style: TextStyle(fontFamily: 'monospace', fontSize: fontSize),
    ),
    textDirection: TextDirection.ltr,
  )..layout();
  final size = Size(
    math.max(1, painter.width),
    (fontSize * 1.5).ceilToDouble(),
  );
  painter.dispose();
  return size;
}

Map<String, dynamic> terminalViewport(Size size, Size cell) {
  final columns = (size.width / cell.width).floor().clamp(1, 500);
  final rows = (size.height / cell.height).floor().clamp(1, 200);
  return {
    'columns': columns,
    'rows': rows,
    'pixel_width': (columns * cell.width).round(),
    'pixel_height': (rows * cell.height).round(),
  };
}

Map<String, dynamic> terminalAppearance(BuildContext context) {
  final theme = Theme.of(context);
  final colors = theme.colorScheme;
  final dark = theme.brightness == Brightness.dark;
  Map<String, int> rgb(Color value) {
    final color = Color.alphaBlend(value, colors.surface);
    return {
      'red': (color.r * 255).round(),
      'green': (color.g * 255).round(),
      'blue': (color.b * 255).round(),
    };
  }

  // ANSI's six chromatic slots use Material swatches; neutral slots and the
  // terminal surface use the same application theme as the surrounding controls.
  final chromatic = [
    Colors.red,
    Colors.green,
    Colors.amber,
    Colors.blue,
    Colors.purple,
    Colors.cyan,
  ];
  return {
    'foreground': rgb(colors.onSurface),
    'background': rgb(colors.surface),
    'color_scheme': dark ? 'dark' : 'light',
    'palette': [
      rgb(colors.surface),
      for (final color in chromatic) rgb(color[dark ? 300 : 700]!),
      rgb(colors.onSurface),
      rgb(colors.onSurfaceVariant),
      for (final color in chromatic) rgb(color[dark ? 200 : 600]!),
      rgb(colors.onSurface),
    ],
  };
}

Map<String, dynamic> terminalLaunch(BuildContext context) {
  final size = MediaQuery.sizeOf(context);
  return {
    'viewport': terminalViewport(
      Size(math.max(1, size.width - 40), math.max(1, size.height - 240)),
      terminalCell(context),
    ),
    'appearance': terminalAppearance(context),
  };
}
