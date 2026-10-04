import 'package:flutter/material.dart';
import 'package:webview_flutter_platform_interface/webview_flutter_platform_interface.dart';

class BrowserPlatform extends WebViewPlatform {
  late BrowserController controller;
  late BrowserNavigation navigation;

  @override
  PlatformWebViewController createPlatformWebViewController(
    PlatformWebViewControllerCreationParams params,
  ) => controller = BrowserController(params);

  @override
  PlatformNavigationDelegate createPlatformNavigationDelegate(
    PlatformNavigationDelegateCreationParams params,
  ) => navigation = BrowserNavigation(params);

  @override
  PlatformWebViewWidget createPlatformWebViewWidget(
    PlatformWebViewWidgetCreationParams params,
  ) => BrowserWidget(params);
}

class BrowserController extends PlatformWebViewController {
  BrowserController(super.params) : super.implementation();
  final requests = <Uri>[];
  int reloads = 0;
  late JavaScriptMode javascript;

  @override
  Future<void> setJavaScriptMode(JavaScriptMode mode) async =>
      javascript = mode;

  @override
  Future<void> setPlatformNavigationDelegate(
    PlatformNavigationDelegate handler,
  ) async {}

  @override
  Future<void> loadRequest(LoadRequestParams params) async =>
      requests.add(params.uri);

  @override
  Future<void> reload() async => reloads++;
}

class BrowserNavigation extends PlatformNavigationDelegate {
  BrowserNavigation(super.params) : super.implementation();
  late PageEventCallback started;
  late PageEventCallback finished;
  late WebResourceErrorCallback error;
  late NavigationRequestCallback request;

  @override
  Future<void> setOnPageStarted(PageEventCallback callback) async =>
      started = callback;

  @override
  Future<void> setOnPageFinished(PageEventCallback callback) async =>
      finished = callback;

  @override
  Future<void> setOnWebResourceError(WebResourceErrorCallback callback) async =>
      error = callback;

  @override
  Future<void> setOnNavigationRequest(
    NavigationRequestCallback callback,
  ) async => request = callback;
}

class BrowserWidget extends PlatformWebViewWidget {
  BrowserWidget(super.params) : super.implementation();

  @override
  Widget build(BuildContext context) => const SizedBox.expand();
}
