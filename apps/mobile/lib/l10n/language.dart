import 'package:flutter/widgets.dart';

enum AppLanguage {
  system('languageSystem', null),
  chinese('languageChinese', Locale('zh')),
  traditionalChinese(
    'languageTraditionalChinese',
    Locale.fromSubtags(languageCode: 'zh', scriptCode: 'Hant'),
  ),
  english('languageEnglish', Locale('en')),
  japanese('languageJapanese', Locale('ja')),
  korean('languageKorean', Locale('ko')),
  french('languageFrench', Locale('fr')),
  german('languageGerman', Locale('de')),
  spanish('languageSpanish', Locale('es')),
  portugueseBrazil('languagePortugueseBrazil', Locale('pt', 'BR')),
  russian('languageRussian', Locale('ru'));

  const AppLanguage(this.labelKey, this.locale);

  final String labelKey;
  final Locale? locale;

  static Locale resolve(List<Locale>? preferred, Iterable<Locale> supported) {
    for (final locale in preferred ?? const <Locale>[]) {
      if (locale.languageCode == 'zh') {
        final traditional =
            locale.scriptCode == 'Hant' ||
            (locale.scriptCode != 'Hans' &&
                const ['TW', 'HK', 'MO'].contains(locale.countryCode));
        return traditional ? traditionalChinese.locale! : chinese.locale!;
      }
      for (final language in values) {
        final candidate = language.locale;
        if (candidate == null ||
            candidate.languageCode != locale.languageCode) {
          continue;
        }
        if (candidate.languageCode != 'pt' || locale.countryCode == 'BR') {
          return candidate;
        }
      }
    }
    return english.locale!;
  }
}
