import React, { useMemo, useState } from 'react';
import { AlertCircle, CheckCircle2, ChevronDown, ChevronUp, Plug } from 'lucide-react';
import { api } from '../api/client';
import { AccountView, ClaudeProviderInput, ProviderTestResult } from '../types';
import { Button } from './common/Button';
import { useI18n } from '../i18n';

type Strings = ReturnType<typeof useI18n>['t']['accounts'];

interface ProviderPreset {
  id: string;
  label: string;
  claude?: { name: string; baseUrl: string; model: string; haiku?: string; sonnet?: string; opus?: string };
  codex?: { name: string; baseUrl: string; model: string };
}

const PROVIDER_PRESETS: ProviderPreset[] = [
  {
    id: 'official',
    label: 'Official',
    claude: { name: 'Anthropic Official', baseUrl: 'https://api.anthropic.com', model: 'claude-sonnet-4-6' },
    codex: { name: 'OpenAI Official', baseUrl: 'https://api.openai.com/v1', model: 'gpt-5' },
  },
  {
    id: 'openrouter',
    label: 'OpenRouter',
    claude: { name: 'OpenRouter', baseUrl: 'https://openrouter.ai/api', model: 'anthropic/claude-sonnet-4' },
    codex: { name: 'OpenRouter', baseUrl: 'https://openrouter.ai/api/v1', model: 'openai/gpt-5' },
  },
  {
    id: 'deepseek',
    label: 'DeepSeek',
    claude: {
      name: 'DeepSeek',
      baseUrl: 'https://api.deepseek.com/anthropic',
      model: 'deepseek-chat',
      haiku: 'deepseek-chat',
      sonnet: 'deepseek-chat',
      opus: 'deepseek-reasoner',
    },
    codex: { name: 'DeepSeek', baseUrl: 'https://api.deepseek.com/v1', model: 'deepseek-chat' },
  },
  {
    id: 'siliconflow',
    label: 'SiliconFlow',
    claude: { name: 'SiliconFlow', baseUrl: 'https://api.siliconflow.cn/', model: 'deepseek-ai/DeepSeek-V3' },
    codex: { name: 'SiliconFlow', baseUrl: 'https://api.siliconflow.cn/v1', model: 'deepseek-ai/DeepSeek-V3' },
  },
  {
    id: 'zhipu',
    label: 'GLM',
    claude: {
      name: 'Zhipu GLM',
      baseUrl: 'https://open.bigmodel.cn/api/anthropic',
      model: 'glm-4.6',
      haiku: 'glm-4.5-air',
      sonnet: 'glm-4.6',
      opus: 'glm-4.6',
    },
    codex: { name: 'Zhipu GLM', baseUrl: 'https://open.bigmodel.cn/api/paas/v4', model: 'glm-4.6' },
  },
  {
    id: 'kimi',
    label: 'Kimi',
    claude: { name: 'Kimi Moonshot', baseUrl: 'https://api.moonshot.cn/anthropic', model: 'kimi-k2-0905-preview' },
    codex: { name: 'Kimi Moonshot', baseUrl: 'https://api.moonshot.cn/v1', model: 'kimi-k2-0905-preview' },
  },
  {
    id: 'minimax',
    label: 'MiniMax',
    claude: { name: 'MiniMax', baseUrl: 'https://api.minimaxi.com/anthropic', model: 'MiniMax-M1' },
    codex: { name: 'MiniMax', baseUrl: 'https://api.minimaxi.com/v1', model: 'MiniMax-M1' },
  },
];

export interface RelayFormProps {
  client?: 'claude' | 'codex';
  account?: AccountView;
  busy: boolean;
  onCancel?: () => void;
  onSave: (input: ClaudeProviderInput) => void | Promise<void>;
}

/** Splits a relay's detail line (`url · model`) back into its parts for editing. */
function splitDetail(detail?: string | null): [string, string] {
  const [url = '', model = ''] = (detail || '').split(' · ');
  return [url, model];
}

export const RelayForm: React.FC<RelayFormProps> = ({
  client = 'claude',
  account,
  busy,
  onCancel,
  onSave,
}) => {
  const { t } = useI18n();
  const a: Strings = t.accounts;
  const [initialUrl, initialModel] = splitDetail(account?.detail);

  const [name, setName] = useState(account?.name || '');
  const [url, setUrl] = useState(account?.base_url ?? initialUrl);
  const [token, setToken] = useState('');
  const [model, setModel] = useState(account?.model ?? initialModel);
  const [authField, setAuthField] = useState(account?.auth_field || 'ANTHROPIC_AUTH_TOKEN');
  const [haikuModel, setHaikuModel] = useState(account?.haiku_model || '');
  const [sonnetModel, setSonnetModel] = useState(account?.sonnet_model || '');
  const [opusModel, setOpusModel] = useState(account?.opus_model || '');
  const [showAdvanced, setShowAdvanced] = useState(
    Boolean(
      (account?.auth_field && account.auth_field !== 'ANTHROPIC_AUTH_TOKEN') ||
        account?.haiku_model ||
        account?.sonnet_model ||
        account?.opus_model
    )
  );
  const [selectedPreset, setSelectedPreset] = useState<string>(account ? 'custom' : '');
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<ProviderTestResult | null>(null);

  const validUrl = useMemo(() => {
    const trimmed = url.trim();
    if (!trimmed) return client === 'codex';
    return /^https?:\/\/\S+$/.test(trimmed);
  }, [url, client]);

  const valid = validUrl && (Boolean(account) || token.trim() !== '');

  const inputClass =
    'w-full bg-slate-50/90 dark:bg-[#0e1523] text-slate-800 dark:text-slate-100 px-3 py-2 rounded-xl border border-slate-200/90 dark:border-slate-700/80 shadow-[inset_0_1px_2px_rgba(15,23,42,0.03)] focus:border-blue-500 focus:ring-2 focus:ring-blue-500/15 focus:bg-white dark:focus:bg-[#141d2f] focus:outline-none placeholder-slate-400 dark:placeholder-slate-500 text-xs transition-all';

  const applyPreset = (presetId: string) => {
    setSelectedPreset(presetId);
    setTestResult(null);
    if (presetId === 'custom') return;
    const preset = PROVIDER_PRESETS.find((item) => item.id === presetId);
    const config = client === 'codex' ? preset?.codex : preset?.claude;
    if (!config) return;
    setName(config.name);
    setUrl(config.baseUrl);
    setModel(config.model);
    if (client === 'claude' && preset?.claude) {
      setHaikuModel(preset.claude.haiku || '');
      setSonnetModel(preset.claude.sonnet || '');
      setOpusModel(preset.claude.opus || '');
      if (preset.claude.haiku || preset.claude.sonnet || preset.claude.opus) {
        setShowAdvanced(true);
      }
    }
  };

  const handleTestConnection = async () => {
    const baseUrl =
      url.trim() || (client === 'codex' ? 'https://api.openai.com/v1' : 'https://api.anthropic.com');
    setTesting(true);
    setTestResult(null);
    try {
      const result = await api.testProviderConnection(
        baseUrl,
        token.trim(),
        client === 'claude' ? authField : null
      );
      setTestResult(result);
    } catch (e) {
      setTestResult({
        ok: false,
        latency_ms: 0,
        models: [],
        message: String(e),
      });
    } finally {
      setTesting(false);
    }
  };

  return (
    <form
      className="p-4 rounded-2xl border border-blue-200/85 dark:border-blue-500/25 bg-gradient-to-b from-blue-50/55 via-white/95 to-white dark:from-blue-500/[0.07] dark:via-[#141d2f]/95 dark:to-[#11192a] shadow-[0_6px_20px_rgba(37,99,235,0.06)] dark:shadow-[0_8px_24px_rgba(0,0,0,0.3)] grid grid-cols-1 md:grid-cols-2 gap-3.5 text-xs"
      onSubmit={(e) => {
        e.preventDefault();
        if (!valid) return;
        onSave({
          id: account?.id ?? null,
          name: name.trim(),
          base_url: url.trim(),
          token: token.trim() || null,
          model: model.trim() || null,
          auth_field: client === 'claude' ? authField : null,
          haiku_model: client === 'claude' ? haikuModel.trim() || null : null,
          sonnet_model: client === 'claude' ? sonnetModel.trim() || null : null,
          opus_model: client === 'claude' ? opusModel.trim() || null : null,
        });
      }}
    >
      <div className="md:col-span-2 flex flex-wrap items-center justify-between gap-2 pb-1 border-b border-slate-200/70 dark:border-slate-800/80">
        <h4 className="text-sm font-semibold text-slate-900 dark:text-white">
          {account ? a.providerFormEdit : a.providerFormNew}
        </h4>
        <div className="flex flex-wrap items-center gap-1">
          <span className="text-[11px] text-slate-500 dark:text-slate-400 mr-1">{a.providerPresets}:</span>
          {PROVIDER_PRESETS.map((preset) => (
            <button
              key={preset.id}
              type="button"
              onClick={() => applyPreset(preset.id)}
              className={`px-2.5 py-0.5 rounded-full text-[11px] font-medium border transition-all ${
                selectedPreset === preset.id
                  ? 'bg-gradient-to-b from-blue-500 to-blue-600 text-white border-blue-600 shadow-[0_1px_3px_rgba(37,99,235,0.28)]'
                  : 'bg-white/80 dark:bg-[#192338] border-slate-200/90 dark:border-slate-700/75 text-slate-700 dark:text-slate-300 hover:border-blue-300 dark:hover:border-blue-500/40 hover:text-blue-600 dark:hover:text-sky-300'
              }`}
            >
              {preset.label}
            </button>
          ))}
          <button
            type="button"
            onClick={() => applyPreset('custom')}
            className={`px-2.5 py-0.5 rounded-full text-[11px] font-medium border transition-all ${
              selectedPreset === 'custom'
                ? 'bg-gradient-to-b from-blue-500 to-blue-600 text-white border-blue-600 shadow-[0_1px_3px_rgba(37,99,235,0.28)]'
                : 'bg-white/80 dark:bg-[#192338] border-slate-200/90 dark:border-slate-700/75 text-slate-500 dark:text-slate-400 hover:text-slate-900 dark:hover:text-white'
            }`}
          >
            {a.presetCustom}
          </button>
        </div>
      </div>

      <label className="flex flex-col gap-1">
        <span className="font-semibold text-slate-600 dark:text-slate-300">{a.relayName}</span>
        <input value={name} onChange={(e) => setName(e.target.value)} className={inputClass} />
      </label>

      <label className="flex flex-col gap-1">
        <span className="font-semibold text-slate-600 dark:text-slate-300">
          {client === 'claude' ? a.relayUrl : a.providerBaseUrl}
        </span>
        <input
          value={url}
          onChange={(e) => {
            setUrl(e.target.value);
            setTestResult(null);
          }}
          placeholder={client === 'codex' ? 'https://api.openai.com/v1' : 'https://'}
          className={`${inputClass} font-mono`}
        />
      </label>

      <label className="flex flex-col gap-1">
        <span className="font-semibold text-slate-600 dark:text-slate-300">
          {client === 'claude' ? a.relayToken : a.providerApiKey}
        </span>
        <input
          type="password"
          autoComplete="off"
          value={token}
          onChange={(e) => {
            setToken(e.target.value);
            setTestResult(null);
          }}
          placeholder={account ? a.relayTokenKeep : 'sk-...'}
          className={`${inputClass} font-mono`}
        />
      </label>

      <label className="flex flex-col gap-1">
        <span className="font-semibold text-slate-600 dark:text-slate-300">
          {client === 'claude' ? a.relayModel : a.providerModel}
        </span>
        <input value={model} onChange={(e) => setModel(e.target.value)} className={`${inputClass} font-mono`} />
      </label>

      {client === 'claude' && (
        <div className="md:col-span-2">
          <button
            type="button"
            onClick={() => setShowAdvanced((v) => !v)}
            className="inline-flex items-center gap-1 text-[11px] font-medium text-blue-600 dark:text-sky-400 hover:underline"
          >
            {showAdvanced ? <ChevronUp size={13} /> : <ChevronDown size={13} />}
            <span>{a.providerAdvancedModels}</span>
          </button>

          {showAdvanced && (
            <div className="mt-2 grid grid-cols-1 md:grid-cols-2 gap-2.5 p-3.5 rounded-xl bg-slate-50/85 dark:bg-[#0e1523]/85 border border-slate-200/80 dark:border-slate-800/85">
              <label className="flex flex-col gap-1">
                <span className="font-semibold text-slate-600 dark:text-slate-300">{a.providerAuthField}</span>
                <select
                  value={authField}
                  onChange={(e) => setAuthField(e.target.value)}
                  className="cv-select w-full font-mono text-xs !h-[34px] !rounded-xl"
                >
                  <option value="ANTHROPIC_AUTH_TOKEN">ANTHROPIC_AUTH_TOKEN</option>
                  <option value="ANTHROPIC_API_KEY">ANTHROPIC_API_KEY</option>
                </select>
              </label>
              <label className="flex flex-col gap-1">
                <span className="font-semibold text-slate-600 dark:text-slate-300">Haiku (ANTHROPIC_DEFAULT_HAIKU_MODEL)</span>
                <input
                  value={haikuModel}
                  onChange={(e) => setHaikuModel(e.target.value)}
                  placeholder="claude-haiku-4-5"
                  className={`${inputClass} font-mono`}
                />
              </label>
              <label className="flex flex-col gap-1">
                <span className="font-semibold text-slate-600 dark:text-slate-300">Sonnet (ANTHROPIC_DEFAULT_SONNET_MODEL)</span>
                <input
                  value={sonnetModel}
                  onChange={(e) => setSonnetModel(e.target.value)}
                  placeholder="claude-sonnet-4-6"
                  className={`${inputClass} font-mono`}
                />
              </label>
              <label className="flex flex-col gap-1">
                <span className="font-semibold text-slate-600 dark:text-slate-300">Opus (ANTHROPIC_DEFAULT_OPUS_MODEL)</span>
                <input
                  value={opusModel}
                  onChange={(e) => setOpusModel(e.target.value)}
                  placeholder="claude-opus-4-6"
                  className={`${inputClass} font-mono`}
                />
              </label>
            </div>
          )}
        </div>
      )}

      {testResult && (
        <div
          className={`md:col-span-2 p-3 rounded-xl border text-xs flex flex-col gap-2 ${
            testResult.ok
              ? 'border-emerald-300/80 dark:border-emerald-500/30 bg-emerald-50/90 dark:bg-emerald-500/10 text-emerald-700 dark:text-emerald-300'
              : 'border-rose-300/80 dark:border-rose-500/30 bg-rose-50/90 dark:bg-rose-500/10 text-rose-700 dark:text-rose-300'
          }`}
        >
          <div className="flex items-center gap-1.5 font-medium">
            {testResult.ok ? <CheckCircle2 size={14} /> : <AlertCircle size={14} />}
            <span>
              {testResult.ok
                ? testResult.models.length > 0
                  ? a.providerTestSuccess
                      .replace('{ms}', String(testResult.latency_ms))
                      .replace('{count}', String(testResult.models.length))
                  : a.providerTestSuccessNoModels
                      .replace('{ms}', String(testResult.latency_ms))
                      .replace('{message}', testResult.message)
                : a.providerTestFail
                    .replace('{ms}', String(testResult.latency_ms))
                    .replace('{message}', testResult.message)}
            </span>
          </div>
          {testResult.ok && testResult.models.length > 0 && (
            <div className="flex flex-wrap items-center gap-1">
              <span className="text-[11px] opacity-80 mr-1">{a.providerPickModel}</span>
              {testResult.models.slice(0, 16).map((item) => (
                <button
                  key={item}
                  type="button"
                  onClick={() => setModel(item)}
                  className={`px-2 py-0.5 rounded-md font-mono text-[11px] border transition ${
                    model === item
                      ? 'bg-blue-600 text-white border-blue-600 shadow-sm'
                      : 'bg-white/85 dark:bg-[#11192a]/80 border-emerald-300/80 dark:border-emerald-500/30 hover:border-blue-500'
                  }`}
                >
                  {item}
                </button>
              ))}
            </div>
          )}
        </div>
      )}

      <div className="md:col-span-2 flex flex-wrap items-center justify-between gap-2">
        <Button
          size="sm"
          variant="secondary"
          icon={<Plug size={13} />}
          onClick={handleTestConnection}
          disabled={busy || testing || !validUrl || !token.trim()}
        >
          {testing ? a.providerTesting : a.providerTestConn}
        </Button>
        <div className="flex items-center gap-2">
          {onCancel && (
            <Button size="sm" variant="ghost" onClick={onCancel}>
              {t.common.cancel}
            </Button>
          )}
          <Button size="sm" variant="primary" type="submit" disabled={busy || !valid}>
            {a.save}
          </Button>
        </div>
      </div>
    </form>
  );
};
