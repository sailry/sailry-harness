import 'api/connection.dart';
import 'api/overview.dart';
import 'src/native.dart';

class Controller extends NativeHandle {
  Controller.internal(super.bridge, super.handle);

  static Future<Controller> open({
    required String path,
    required bool internet,
    required List<String> relays,
  }) async {
    await initializeNativeBridge();
    final bridge = nativeBridge;
    final value = await bridge.invoke(0, 'open', {
      'path': path,
      'internet': internet,
      'relays': relays,
    });
    return Controller.internal(bridge, nativeHandle(value, 'controller'));
  }

  Future<void> close() => closeNative();

  Future<String> id() async => await invoke('id') as String;

  Future<void> setName({required String name}) async {
    await invoke('set_name', {'name': name});
  }

  Future<String> pair({required String ticket}) async =>
      await invoke('pair', {'ticket': ticket}) as String;

  Future<String> pairPin({required String origin, required String pin}) async =>
      await invoke('pair_pin', {'origin': origin, 'pin': pin}) as String;

  Future<List<String>> peers() async =>
      (await invoke('peers') as List<dynamic>).cast<String>();

  Future<void> forget({required String address}) async {
    await invoke('forget', {'address': address});
  }

  Future<void> networkChanged() async {
    await invoke('network_changed');
  }

  Future<Connection> connect({required String address}) async {
    final value = await invoke('connect', {'address': address});
    return Connection.internal(bridge, nativeHandle(value, 'connection'));
  }

  Future<String> notifications() async =>
      await invoke('notifications') as String;

  Future<OverviewUpdates> watchUsage({
    required List<String> addresses,
    required String query,
  }) async {
    final value = await invoke('watch_usage', {
      'addresses': addresses,
      'query': query,
    });
    return OverviewUpdates.internal(
      bridge,
      nativeHandle(value, 'overview_updates'),
    );
  }
}
