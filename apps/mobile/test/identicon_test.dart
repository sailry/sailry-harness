import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_svg/flutter_svg.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/ui/identicon.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  test('identities match the desktop FNV definition', () {
    for (final seed in [
      '',
      'agent-7',
      '01a197bb-2af3-7390-a524-3c1f143d01c4',
      '角色-研究',
      for (var index = 0; index < 256; index++) 'agent-$index',
    ]) {
      var hash = BigInt.parse('cbf29ce484222325', radix: 16);
      final prime = BigInt.parse('100000001b3', radix: 16);
      final mask = (BigInt.one << 64) - BigInt.one;
      for (final byte in utf8.encode(seed)) {
        hash = ((hash ^ BigInt.from(byte)) * prime) & mask;
      }
      final expected = (
        (hash % BigInt.from(8)).toInt(),
        ((hash >> 16) % BigInt.from(8)).toInt(),
      );
      expect(Identicon.identity(seed), expected, reason: seed);
    }
    expect(Identicon.identity('agent-7'), (2, 0));
  });

  test('all shapes decode to the desktop viewport', () async {
    for (var shape = 0; shape < 8; shape++) {
      final picture = await vg.loadPicture(
        SvgStringLoader(Identicon.source(shape)),
        null,
      );
      expect(picture.size, const Size(24, 24), reason: 'Shape $shape');
      picture.picture.dispose();
    }
  });
}
