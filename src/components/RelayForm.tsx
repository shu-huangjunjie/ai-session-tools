import React, { useState } from 'react';
import { AccountView } from '../types';
import { Button } from './common/Button';
import { useI18n } from '../i18n';

type Strings = ReturnType<typeof useI18n>['t']['accounts'];

export interface RelayFormProps {
  account?: AccountView;
  busy: boolean;
  onCancel: () => void;
  onSave: (input: { id?: string | null; name: string; base_url: string; token?: string | null; model?: string | null }) => void;
}

/** Splits a relay's detail line (`url · model`) back into its parts for editing. */
function splitDetail(detail?: string | null): [string, string] {
  const [url = '', model = ''] = (detail || '').split(' · ');
  return [url, model];
}

export const RelayForm: React.FC<RelayFormProps> = ({ account, busy, onCancel, onSave }) => {
  const { t } = useI18n();
  const a: Strings = t.accounts;
  const [initialUrl, initialModel] = splitDetail(account?.detail);
  const [name, setName] = useState(account?.name || '');
  const [url, setUrl] = useState(initialUrl);
  const [token, setToken] = useState('');
  const [model, setModel] = useState(initialModel);
  const valid = /^https?:\/\/\S+$/.test(url.trim()) && (Boolean(account) || token.trim() !== '');
  const inputClass =
    'w-full bg-black/[0.04] dark:bg-white/[0.06] px-3 py-2 rounded-xl border border-black/[0.08] dark:border-white/[0.08] focus:border-[#0071e3] focus:outline-none placeholder-[#86868b]';

  return (
    <form
      className="p-4 rounded-2xl border border-[#0071e3]/30 bg-[#0071e3]/[0.03] grid grid-cols-1 md:grid-cols-2 gap-3"
      onSubmit={(e) => {
        e.preventDefault();
        if (valid) onSave({ id: account?.id ?? null, name: name.trim(), base_url: url.trim(), token: token.trim() || null, model: model.trim() || null });
      }}
    >
      <h4 className="md:col-span-2 text-sm font-semibold text-[#1d1d1f] dark:text-white">{account ? a.relayFormEdit : a.relayFormNew}</h4>
      <label className="flex flex-col gap-1">
        <span className="font-semibold text-[#86868b]">{a.relayName}</span>
        <input value={name} onChange={(e) => setName(e.target.value)} className={inputClass} />
      </label>
      <label className="flex flex-col gap-1">
        <span className="font-semibold text-[#86868b]">{a.relayUrl}</span>
        <input value={url} onChange={(e) => setUrl(e.target.value)} placeholder="https://" className={`${inputClass} font-mono`} />
      </label>
      <label className="flex flex-col gap-1">
        <span className="font-semibold text-[#86868b]">{a.relayToken}</span>
        <input type="password" autoComplete="off" value={token} onChange={(e) => setToken(e.target.value)} placeholder={account ? a.relayTokenKeep : ''} className={`${inputClass} font-mono`} />
      </label>
      <label className="flex flex-col gap-1">
        <span className="font-semibold text-[#86868b]">{a.relayModel}</span>
        <input value={model} onChange={(e) => setModel(e.target.value)} className={`${inputClass} font-mono`} />
      </label>
      <div className="md:col-span-2 flex items-center justify-end gap-2">
        <Button size="sm" variant="ghost" onClick={onCancel}>
          {t.common.cancel}
        </Button>
        <Button size="sm" variant="primary" type="submit" disabled={busy || !valid}>
          {a.save}
        </Button>
      </div>
    </form>
  );
};
