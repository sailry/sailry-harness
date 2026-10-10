import 'dart:convert';
import 'dart:math' as math;

enum ChartType { line, bar, area, pie, radar, candlestick, sankey }

typedef ChartPoint = ({String label, double value});
typedef ChartCandle = ({
  String label,
  double open,
  double high,
  double low,
  double close,
});
typedef ChartNode = ({String id, String label});
typedef ChartLink = ({String source, String target, double value});

/// The closed presentation schema in crates/content/src/markdown/charts.rs.
class ChartSpec {
  ChartSpec._(
    this.type,
    this.title, {
    List<ChartPoint> points = const [],
    List<ChartCandle> candles = const [],
    List<ChartNode> nodes = const [],
    List<ChartLink> links = const [],
  }) : points = List.unmodifiable(points),
       candles = List.unmodifiable(candles),
       nodes = List.unmodifiable(nodes),
       links = List.unmodifiable(links);

  final ChartType type;
  final String title;
  final List<ChartPoint> points;
  final List<ChartCandle> candles;
  final List<ChartNode> nodes;
  final List<ChartLink> links;

  static ChartSpec? parse(String source) {
    if (source.length > 64 * 1024 || utf8.encode(source).length > 64 * 1024) {
      return null;
    }
    Object? decoded;
    try {
      decoded = jsonDecode(source);
    } on FormatException {
      return null;
    }
    if (decoded is! Map<String, dynamic>) return null;
    final title = decoded.containsKey('title') ? decoded['title'] : '';
    if (!_label(title)) return null;
    final rawType = decoded['type'];
    final type = ChartType.values
        .where((value) => value.name == rawType)
        .firstOrNull;
    if (type == null) return null;
    if (!_keys(decoded, {
      'type',
      'title',
      if (type == ChartType.sankey) ...['nodes', 'links'] else 'data',
    })) {
      return null;
    }
    if (type == ChartType.sankey) {
      return _graph(decoded, title as String);
    }
    final data = decoded['data'];
    if (data is! List || data.isEmpty || data.length > 500) return null;
    if (type == ChartType.candlestick) {
      final candles = <ChartCandle>[];
      for (final item in data) {
        if (item is! Map<String, dynamic> ||
            !_keys(item, {'label', 'open', 'high', 'low', 'close'}) ||
            !_label(item['label']) ||
            ![
              'open',
              'high',
              'low',
              'close',
            ].every((key) => _number(item[key]))) {
          return null;
        }
        final candle = (
          label: item['label'] as String,
          open: (item['open'] as num).toDouble(),
          high: (item['high'] as num).toDouble(),
          low: (item['low'] as num).toDouble(),
          close: (item['close'] as num).toDouble(),
        );
        if (candle.low > math.min(candle.open, candle.close) ||
            candle.high < math.max(candle.open, candle.close)) {
          return null;
        }
        candles.add(candle);
      }
      return ChartSpec._(type, title as String, candles: candles);
    }
    final points = <ChartPoint>[];
    for (final item in data) {
      if (item is! Map<String, dynamic> ||
          !_keys(item, {'label', 'value'}) ||
          !_label(item['label']) ||
          !_number(item['value'])) {
        return null;
      }
      points.add((
        label: item['label'] as String,
        value: (item['value'] as num).toDouble(),
      ));
    }
    if (type == ChartType.pie || type == ChartType.radar) {
      if (points.any((point) => point.value < 0) ||
          !points.any((point) => point.value > 0) ||
          type == ChartType.radar && points.length < 3) {
        return null;
      }
    }
    return ChartSpec._(type, title as String, points: points);
  }

  String get description => switch (type) {
    ChartType.candlestick =>
      candles
          .map(
            (candle) =>
                '${candle.label}: ${candle.open} / ${candle.high} / ${candle.low} / ${candle.close}',
          )
          .join('; '),
    ChartType.sankey =>
      links
          .map((link) => '${link.source} → ${link.target}: ${link.value}')
          .join('; '),
    _ => points.map((point) => '${point.label}: ${point.value}').join('; '),
  };
}

bool _keys(Map<String, dynamic> value, Set<String> allowed) =>
    value.keys.every(allowed.contains);

bool _label(Object? value) =>
    value is String &&
    utf8.encode(value).length <= 256 &&
    !value.runes.any(
      (rune) =>
          rune <= 0x1f ||
          rune >= 0x7f && rune <= 0x9f ||
          // serde_json rejects unpaired surrogate escapes.
          rune >= 0xd800 && rune <= 0xdfff,
    );

bool _number(Object? value) =>
    value is num && value.isFinite && value.abs() <= 1e12;

ChartSpec? _graph(Map<String, dynamic> data, String title) {
  final rawNodes = data['nodes'];
  final rawLinks = data['links'];
  if (rawNodes is! List ||
      rawNodes.isEmpty ||
      rawNodes.length > 64 ||
      rawLinks is! List ||
      rawLinks.isEmpty ||
      rawLinks.length > 128) {
    return null;
  }
  final nodes = <ChartNode>[];
  final ids = <String, int>{};
  for (final node in rawNodes) {
    if (node is! Map<String, dynamic> ||
        !_keys(node, {'id', 'label'}) ||
        !_label(node['id']) ||
        node['id'] == '' ||
        !_label(node['label']) ||
        ids.containsKey(node['id'])) {
      return null;
    }
    ids[node['id'] as String] = nodes.length;
    nodes.add((id: node['id'] as String, label: node['label'] as String));
  }
  final links = <ChartLink>[];
  final edges = List.generate(nodes.length, (_) => <int>[]);
  final degrees = List.filled(nodes.length, 0);
  for (final link in rawLinks) {
    if (link is! Map<String, dynamic> ||
        !_keys(link, {'source', 'target', 'value'}) ||
        !ids.containsKey(link['source']) ||
        !ids.containsKey(link['target']) ||
        link['source'] == link['target'] ||
        !_number(link['value']) ||
        (link['value'] as num) <= 0) {
      return null;
    }
    final source = ids[link['source']]!;
    final target = ids[link['target']]!;
    edges[source].add(target);
    degrees[target]++;
    links.add((
      source: link['source'] as String,
      target: link['target'] as String,
      value: (link['value'] as num).toDouble(),
    ));
  }
  final ready = [
    for (var index = 0; index < degrees.length; index++)
      if (degrees[index] == 0) index,
  ];
  var visited = 0;
  while (ready.isNotEmpty) {
    final node = ready.removeLast();
    visited++;
    for (final target in edges[node]) {
      if (--degrees[target] == 0) ready.add(target);
    }
  }
  if (visited != nodes.length) return null;
  return ChartSpec._(ChartType.sankey, title, nodes: nodes, links: links);
}
