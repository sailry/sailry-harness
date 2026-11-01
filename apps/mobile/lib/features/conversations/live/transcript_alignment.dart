import 'package:flutter/foundation.dart' show precisionErrorTolerance;
import 'package:flutter/rendering.dart';
import 'package:flutter/widgets.dart';

/// Places a sliver shorter than the viewport against the viewport's far edge.
///
/// A reversed transcript grows from the bottom, yet a short conversation reads
/// from the top. The leading gap fills only the space the content leaves, so
/// the list stays lazy instead of shrink-wrapping every turn.
class SliverAlignEnd extends SingleChildRenderObjectWidget {
  const SliverAlignEnd({super.key, required Widget sliver})
    : super(child: sliver);

  @override
  RenderObject createRenderObject(BuildContext context) =>
      _RenderSliverAlignEnd();
}

class _RenderSliverAlignEnd extends RenderSliverEdgeInsetsPadding {
  EdgeInsets _gap = EdgeInsets.zero;

  @override
  EdgeInsets? get resolvedPadding => _gap;

  @override
  void performLayout() {
    _gap = EdgeInsets.zero;
    super.performLayout();
    final content = child?.geometry;
    if (content == null || content.scrollOffsetCorrection != null) return;
    final gap =
        constraints.viewportMainAxisExtent -
        constraints.precedingScrollExtent -
        content.scrollExtent;
    if (gap <= precisionErrorTolerance) return;
    // The gap sits on the side nearest scroll offset zero.
    _gap = switch (applyGrowthDirectionToAxisDirection(
      constraints.axisDirection,
      constraints.growthDirection,
    )) {
      AxisDirection.up => EdgeInsets.only(bottom: gap),
      AxisDirection.down => EdgeInsets.only(top: gap),
      AxisDirection.left => EdgeInsets.only(right: gap),
      AxisDirection.right => EdgeInsets.only(left: gap),
    };
    super.performLayout();
  }
}
