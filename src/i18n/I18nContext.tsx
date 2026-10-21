import React, { createContext, useContext, useState } from 'react';
import { Locale, Translations, translations } from './translations';

interface I18nContextType {
  locale: Locale;
  setLocale: (locale: Locale) => void;
  toggleLocale: () => void;
  t: Translations;
  interpolate: (template: string, vars: Record<string, string | number>) => string;
}

const I18nContext = createContext<I18nContextType | null>(null);

export const I18nProvider: React.FC<{ children: React.ReactNode }> = ({ children }) => {
  const [locale, setLocaleState] = useState<Locale>(() => {
    const saved = localStorage.getItem('cv_locale');
    if (saved === 'en' || saved === 'zh') {
      return saved;
    }
    // Check browser language
    if (typeof navigator !== 'undefined' && navigator.language && !navigator.language.startsWith('zh')) {
      return 'en';
    }
    return 'zh';
  });

  const setLocale = (newLocale: Locale) => {
    setLocaleState(newLocale);
    localStorage.setItem('cv_locale', newLocale);
  };

  const toggleLocale = () => {
    setLocale(locale === 'zh' ? 'en' : 'zh');
  };

  const interpolate = (template: string, vars: Record<string, string | number>): string => {
    return template.replace(/\{(\w+)\}/g, (_, key) => {
      return vars[key] !== undefined ? String(vars[key]) : `{${key}}`;
    });
  };

  const t = translations[locale] || translations.zh;

  return (
    <I18nContext.Provider value={{ locale, setLocale, toggleLocale, t, interpolate }}>
      {children}
    </I18nContext.Provider>
  );
};

export const useI18n = (): I18nContextType => {
  const context = useContext(I18nContext);
  if (!context) {
    throw new Error('useI18n must be used within an I18nProvider');
  }
  return context;
};
