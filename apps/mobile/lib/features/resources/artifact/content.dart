import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:html/dom.dart' as dom;
import 'package:html/parser.dart' as parser;

import '../../../runtime/json.dart';
import '../../../ui/theme.dart';

const artifactLimit = 512 * 1024;
const artifactViewLimit = 256 * 1024;

Map<String, String> artifactTheme(BuildContext context) {
  final theme = Theme.of(context);
  final colors = theme.colorScheme;
  String css(Color color) {
    final value = color.toARGB32().toRadixString(16).padLeft(8, '0');
    return '#${value.substring(2)}${value.substring(0, 2)}';
  }

  return {
    'background': css(colors.surface),
    'foreground': css(colors.onSurface),
    'diagram_foreground': css(colors.onSurface),
    'diagram_background': 'transparent',
    'muted': css(colors.surfaceContainerHighest),
    'muted_foreground': css(colors.onSurfaceVariant),
    'border': css(colors.outlineVariant),
    'primary': css(colors.secondary),
    'primary_foreground': css(colors.onSecondary),
    'accent': css(colors.tertiary),
    'secondary': css(colors.surfaceContainerHigh),
    'secondary_foreground': css(colors.onSurface),
    'success': css(SailryTheme.statusColor(context, StatusTone.success)),
    'warning': css(SailryTheme.statusColor(context, StatusTone.warning)),
    'danger': css(colors.error),
    'font_family': theme.textTheme.bodyMedium?.fontFamily ?? 'system-ui',
    'mono_font_family': 'monospace',
    'font_size': '${theme.textTheme.bodyMedium?.fontSize ?? 14}px',
    'radius': '12px',
  };
}

// The frame's own policy only tightens the inherited one; frame-src on the page
// also blocks navigation of the authored frame.
const _previewPolicy =
    "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data: blob:; font-src data:; connect-src 'none'; form-action 'none'; worker-src 'none'; object-src 'none'; base-uri 'none'; webrtc 'block'";
const _previewTheme =
    "addEventListener('message',event=>{const rule=event.data&&event.data.sailryTheme;if(event.source!==parent||typeof rule!=='string')return;const style=document.getElementById('sailry-theme');if(style)style.textContent=rule;});";

/// Theme values as `--sailry-*` variables that authored CSS may override.
String artifactThemeRule(Map<String, String> theme, {required bool dark}) {
  final rule = StringBuffer(':root{color-scheme:${dark ? 'dark' : 'light'};');
  for (final entry in theme.entries) {
    if (!RegExp(r'^[a-z_]+$').hasMatch(entry.key)) continue;
    final value = entry.value.replaceAll('<', r'\3c ').replaceAll('>', r'\3e ');
    rule.write('--sailry-${entry.key.replaceAll('_', '-')}:$value;');
  }
  rule.write('}');
  return rule.toString();
}

/// Self-contained preview code runs in an opaque-origin frame, without the
/// package template or any page channel, matching the Desktop wrapper.
String artifactPreviewPage(
  String html,
  Map<String, String> theme, {
  required bool dark,
}) {
  if (html.trim().isEmpty ||
      html.contains('\u0000') ||
      utf8.encode(html).length > artifactLimit) {
    throw const FormatException('Invalid artifact preview');
  }
  final rule = artifactThemeRule(theme, dark: dark);
  final frame =
      '<meta http-equiv="Content-Security-Policy" content="$_previewPolicy; frame-src \'none\'">'
      '<meta name=color-scheme content=${dark ? 'dark' : 'light'}>'
      '<style id=sailry-theme>$rule</style><script>$_previewTheme</script>$html';
  final escaped = frame
      .replaceAll('&', '&amp;')
      .replaceAll('"', '&quot;')
      .replaceAll('<', '&lt;')
      .replaceAll('>', '&gt;');
  return '<!doctype html><html><head><meta charset=utf-8>'
      '<meta name=viewport content="width=device-width,initial-scale=1">'
      '<meta http-equiv="Content-Security-Policy" content="$_previewPolicy; frame-src about:">'
      '<style id=sailry-theme>$rule</style>'
      '<style>html,body{margin:0;height:100%;overflow:hidden;background:var(--sailry-background);color:var(--sailry-foreground)}'
      'iframe{position:absolute;inset:0;border:0;width:100%;height:100%}</style></head>'
      '<body><iframe id=preview sandbox="allow-scripts" '
      'allow="camera \'none\'; microphone \'none\'; geolocation \'none\'; clipboard-read \'none\'" '
      'referrerpolicy="no-referrer" srcdoc="$escaped"></iframe></body></html>';
}

/// Moves a mounted preview to new theme values without reloading authored state.
String artifactPreviewRetheme(Map<String, String> theme, {required bool dark}) {
  final rule = jsonEncode(artifactThemeRule(theme, dark: dark));
  return "(()=>{const rule=$rule;const style=document.getElementById('sailry-theme');"
      "if(style)style.textContent=rule;"
      "document.getElementById('preview')?.contentWindow?.postMessage({sailryTheme:rule},'*');})();";
}

String artifactAnswers(
  Map<String, dynamic> event,
  Map<String, dynamic> artifact,
  String heading,
) {
  if (event['artifact'] != artifact['id'] ||
      event['session'] != artifact['session'] ||
      event['revision'] != artifact['revision'] ||
      event['kind'] != 'submit' ||
      utf8.encode(jsonEncode(event)).length > 65536) {
    throw const FormatException('Invalid artifact submission identity');
  }
  final form = text(event['form']);
  final fields = objects(event['fields']);
  if (form.isEmpty ||
      form.length > 128 ||
      fields.isEmpty ||
      fields.length > 64) {
    throw const FormatException('Invalid artifact form');
  }
  final answers = <String>[];
  for (final field in fields) {
    final label = text(field['label']);
    final name = text(field['name']);
    final values = objects(field['values']);
    if (label.length > 4096 ||
        name.isEmpty ||
        name.length > 128 ||
        values.length > 128) {
      throw const FormatException('Invalid artifact field');
    }
    final selected = <String>[];
    for (final value in values) {
      final raw = text(value['value']);
      final caption = text(value['label'], raw);
      if (raw.length > 4096 || caption.length > 4096) {
        throw const FormatException('Artifact answer exceeds its size limit');
      }
      selected.add(caption == raw ? caption : '$caption ($raw)');
    }
    answers.add('- ${label.isEmpty ? name : label}: ${selected.join(', ')}');
  }
  return '$heading\n${answers.join('\n')}';
}

Future<String> artifactPage(
  Map<String, dynamic> content,
  Map<String, String> theme,
  Map<String, String> labels, {
  required bool writable,
}) async {
  final view = object(content['view']);
  final template = text(view['template']);
  final styles = (view['styles'] as List? ?? []).cast<String>();
  final scripts = (view['scripts'] as List? ?? []).cast<String>();
  final libraries = (view['libraries'] as List? ?? []).cast<String>();
  if (template.isEmpty ||
      styles.length > 8 ||
      scripts.length > 8 ||
      libraries.length > 3 ||
      libraries.toSet().length != libraries.length ||
      !libraries.every(['mermaid', 'echarts', 'mathjax'].contains) ||
      utf8.encode([template, ...styles, ...scripts].join()).length >
          artifactViewLimit ||
      utf8.encode(text(content['html'])).length > artifactLimit) {
    throw const FormatException('Invalid artifact view');
  }
  final sources = <String>[];
  for (final library in libraries) {
    if (library == 'mathjax') {
      for (final file in ['config', 'mathjax', 'fonts', 'render']) {
        final script = await rootBundle.loadString(
          'assets/visuals/$file.js',
          cache: false,
        );
        sources.add(
          file == 'config'
              ? '$script\nMathJax.loader.paths={mathjax:"sailry-visual://localhost"};'
              : script,
        );
      }
    } else {
      sources.add(
        await rootBundle.loadString('assets/visuals/$library.js', cache: false),
      );
    }
  }
  final document = parser.parse(template);
  final head = document.head!;
  // The host owns confinement before any package code or markup is parsed.
  head.nodes.insert(
    0,
    dom.Element.tag('meta')
      ..attributes['http-equiv'] = 'Content-Security-Policy'
      ..attributes['content'] =
          "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data: blob:; font-src data:; connect-src 'none'; form-action 'none'; base-uri 'none'; frame-src 'none'; object-src 'none'",
  );
  head.append(dom.Element.tag('style')..text = styles.join('\n'));
  final artifact = object(content['artifact']);
  final configuration = {
    'html': text(content['html']),
    'theme': theme,
    'labels': labels,
    'writable': writable,
    'identity': {
      'artifact': artifact['id'],
      'revision': artifact['revision'],
      'title': artifact['title'],
      'session': artifact['session'],
    },
  };
  final bootstrap =
      '''
window.sailryDocumentPost = event => SailryDocument.postMessage(JSON.stringify(event));
Promise.resolve(window.sailryDocument.mount(${jsonEncode(configuration)})).then(
()=>window.sailryDocumentPost({kind:'ready'}),
error=>window.sailryDocumentPost({kind:'error',message:String(error?.message||error).slice(0,2048)}));
''';
  final script = [...sources, ...scripts, bootstrap]
      .join('\n')
      .replaceAll(RegExp('</script', caseSensitive: false), r'<\/script');
  document.body!.append(dom.Element.tag('script')..text = script);
  return '<!doctype html>${document.documentElement!.outerHtml}';
}

String exportedArtifact(
  String markup,
  Map<String, dynamic> content,
  Map<String, String> theme, {
  String logo = '',
  String qr = '',
}) {
  if (utf8.encode(markup).length > 4 * 1024 * 1024) {
    throw const FormatException('Artifact export exceeds its size limit');
  }
  final document = parser.parse(
    '<!doctype html><html><head></head><body></body></html>',
  );
  final head = document.head!;
  head.append(dom.Element.tag('meta')..attributes['charset'] = 'utf-8');
  head.append(
    dom.Element.tag('meta')
      ..attributes['name'] = 'viewport'
      ..attributes['content'] = 'width=device-width,initial-scale=1',
  );
  head.append(
    dom.Element.tag('title')..text = text(object(content['artifact'])['title']),
  );
  final css = (object(content['view'])['styles'] as List? ?? [])
      .cast<String>()
      .join('\n');
  final variables = theme.entries
      .map(
        (item) => '--document-${item.key.replaceAll('_', '-')}:${item.value}',
      )
      .join(';');
  head.append(
    dom.Element.tag('style')
      ..text =
          ':root{$variables}\n$css\n'
          '.document-brand{display:flex;align-items:center;gap:12px;max-width:960px;margin:48px auto 0;padding-top:20px;border-top:1px solid var(--document-border)}'
          '.document-brand svg{width:48px;height:48px}.document-brand a{margin-left:auto}.document-brand a svg{width:72px;height:72px}',
  );
  final fragment = parser.parseFragment(markup);
  for (final script in fragment.querySelectorAll('script')) {
    script.remove();
  }
  document.body!.append(fragment);
  if (logo.isNotEmpty && qr.isNotEmpty) {
    final footer = dom.Element.tag('footer')..classes.add('document-brand');
    footer.append(parser.parseFragment(logo));
    footer.append(dom.Element.tag('strong')..text = 'Sailry');
    footer.append(
      dom.Element.tag('a')
        ..attributes['href'] = 'https://sailry.dev'
        ..attributes['aria-label'] = 'Sailry'
        ..append(parser.parseFragment(qr)),
    );
    document.body!.append(footer);
  }
  return '<!doctype html>${document.documentElement!.outerHtml}';
}
