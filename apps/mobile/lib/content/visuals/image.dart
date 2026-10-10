import 'dart:typed_data';

import 'package:flutter/widgets.dart';
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
