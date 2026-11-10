import 'dart:convert';
import 'dart:typed_data';

import 'package:html/parser.dart' as html;

enum VisualKind { mermaid, inlineMath, displayMath }

class VisualRequest {
  const VisualRequest({
    required this.kind,
    required this.source,
    required this.dark,
    required this.foreground,
    required this.background,
    required this.accent,
    required this.fontSize,
  });
  final VisualKind kind;
  final String source;
  final bool dark;
  final String foreground;
  final String background;
  final String accent;
  final double fontSize;

  Map<String, Object> toJson() => {
    'kind': switch (kind) {
      VisualKind.mermaid => 'mermaid',
      VisualKind.inlineMath => 'inline_math',
      VisualKind.displayMath => 'display_math',
    },
    'source': source,
    'dark': dark,
    'foreground': foreground,
    'background': background,
    'accent': accent,
    'font_size': fontSize,
  };

  void validate() {
    final color = RegExp(r'^#[0-9a-fA-F]{6}$');
    if (source.isEmpty ||
        utf8.encode(source).length > 65536 ||
        !fontSize.isFinite ||
        fontSize < 8 ||
        fontSize > 96 ||
        ![foreground, background, accent].every(color.hasMatch)) {
      throw const FormatException('Invalid visual request');
    }
  }
}

class VisualSvg {
  const VisualSvg({
    required this.source,
    required this.width,
    required this.height,
    this.baseline,
    this.png,
  });
  final String source;
  final double width;
  final double height;
  final double? baseline;
  final Uint8List? png;

  factory VisualSvg.fromJson(Map<String, dynamic> value) => VisualSvg(
    source: value['source'] as String,
    width: (value['width'] as num).toDouble(),
    height: (value['height'] as num).toDouble(),
    baseline: (value['baseline'] as num?)?.toDouble(),
    png: value['png'] == null ? null : base64Decode(value['png'] as String),
  );

  void validate() {
    if (utf8.encode(source).length > 2097152 ||
        ![
          width,
          height,
        ].every((value) => value.isFinite && value > 0 && value <= 4096) ||
        (baseline != null &&
            (!baseline!.isFinite || baseline! < 0 || baseline! > height))) {
      throw const FormatException('Rendered visual exceeds its size limit');
    }
    final bytes = png;
    if (bytes != null) {
      const signature = [137, 80, 78, 71, 13, 10, 26, 10];
      if (bytes.length < 24 ||
          bytes.length > 4194304 ||
          !List.generate(
            8,
            (index) => bytes[index] == signature[index],
          ).every((same) => same) ||
          ascii.decode(bytes.sublist(12, 16), allowInvalid: true) != 'IHDR') {
        throw const FormatException('Rendered diagram is not a supported PNG');
      }
      final header = ByteData.sublistView(bytes);
      final width = header.getUint32(16);
      final height = header.getUint32(20);
      if (width == 0 ||
          height == 0 ||
          width > 4096 ||
          height > 4096 ||
          width * height > 2097152) {
        throw const FormatException('Rendered diagram exceeds its pixel limit');
      }
    }
    final fragment = html.parseFragment(source);
    if (fragment.children.length != 1 ||
        fragment.children.single.localName != 'svg') {
      throw const FormatException('Rendered visual is not SVG');
    }
    const unsupported = {
      'script',
      'foreignobject',
      'iframe',
      'image',
      'a',
      'animate',
      'animatetransform',
      'set',
    };
    for (final element in fragment.querySelectorAll('*')) {
      if (unsupported.contains(element.localName?.toLowerCase())) {
        throw const FormatException(
          'Rendered visual contains unsupported elements',
        );
      }
      for (final attribute in element.attributes.entries) {
        final name = attribute.key.toString().toLowerCase();
        final value = attribute.value.trim();
        if (name.startsWith('on') ||
            ((name == 'href' || name.endsWith(':href') || name == 'src') &&
                !value.startsWith('#')) ||
            _externalStyle(value)) {
          throw const FormatException(
            'Rendered visual contains external resources',
          );
        }
      }
      if (element.localName == 'style' && _externalStyle(element.text)) {
        throw const FormatException('Rendered visual contains external styles');
      }
    }
  }
}

bool _externalStyle(String value) {
  final lower = value.toLowerCase();
  if (lower.contains('@import')) return true;
  for (final match in RegExp(r'url\(([^)]*)\)').allMatches(lower)) {
    final target = match[1]!.trim().replaceAll(RegExp(r'''^['"]|['"]$'''), '');
    if (!target.startsWith('#')) return true;
  }
  return lower.contains('url(') && !RegExp(r'url\([^)]*\)').hasMatch(lower);
}
