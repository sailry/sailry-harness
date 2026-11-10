import '../src/native.dart';

String projectModes({required String config, required String modes}) =>
    nativeBridge.invokeSync('project_modes', {
          'config': config,
          'modes': modes,
        })
        as String;

String? selectMode({
  required String config,
  required String modes,
  required String key,
}) =>
    nativeBridge.invokeSync('select_mode', {
          'config': config,
          'modes': modes,
          'key': key,
        })
        as String?;

String? sessionLink({required String link}) =>
    nativeBridge.invokeSync('session_link', {'link': link}) as String?;
