import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/resources/files/breadcrumb.dart';

void main() {
  test("list breadcrumb ancestors from the root", () {
    expect(crumbs(''), [(label: '', path: '')]);
    expect(crumbs('apps/mobile'), [
      (label: '', path: ''),
      (label: 'apps', path: 'apps'),
      (label: 'mobile', path: 'apps/mobile'),
    ]);
  });
}
