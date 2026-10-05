import 'package:flutter/widgets.dart';

enum AppLanguage {
  system('languageSystem', null),
  chinese('languageChinese', Locale('zh')),
  english('languageEnglish', Locale('en'));

  const AppLanguage(this.labelKey, this.locale);

  final String labelKey;
  final Locale? locale;
}
