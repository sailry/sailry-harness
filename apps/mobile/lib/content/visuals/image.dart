import 'dart:typed_data';

import 'package:flutter/widgets.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter_svg/flutter_svg.dart';

/// The conversation owns formula source retention, rather than the global cache.
class FormulaLoader extends SvgStringLoader {
  const FormulaLoader(super.source);

  @override
  Future<ByteData> loadBytes(BuildContext? context) {
    final key = cacheKey(context);
    return super.loadBytes(context).then((data) {
      svg.cache.evict(key);
      return data;
    });
  }
}

/// Baseline shifts non-text children and can discard their top extent. This
/// adapter retains the image box while reporting MathJax's actual baseline.
class FormulaBaseline extends SingleChildRenderObjectWidget {
  const FormulaBaseline({
    super.key,
    required this.baseline,
    required super.child,
  });
  final double baseline;

  @override
  RenderProxyBox createRenderObject(BuildContext context) =>
      _FormulaBaseline(baseline);

  @override
  void updateRenderObject(BuildContext context, RenderProxyBox renderObject) =>
      (renderObject as _FormulaBaseline).baseline = baseline;
}

class _FormulaBaseline extends RenderProxyBox {
  _FormulaBaseline(this._baseline);
  double _baseline;

  set baseline(double value) {
    if (_baseline == value) return;
    _baseline = value;
    markNeedsLayout();
  }

  @override
  double computeDistanceToActualBaseline(TextBaseline baseline) => _baseline;

  @override
  double computeDryBaseline(
    BoxConstraints constraints,
    TextBaseline baseline,
  ) => _baseline;
}
