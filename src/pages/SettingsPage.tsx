import { useEffect, useMemo, useState } from "react";
import { ApiOutlined, FolderOpenOutlined, GlobalOutlined, RollbackOutlined } from "@ant-design/icons";
import {
  App as AntApp,
  Button,
  Checkbox,
  Descriptions,
  Form,
  Input,
  InputNumber,
  Radio,
  Segmented,
  Select,
  Space,
  Tag,
  Typography,
} from "antd";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { LogViewer, Page, Panel, ThemePicker, Workspace, useLogger, useTheme, type ThemeMode } from "../ui-kit";
import { BrandMark } from "../components/BrandMark";
import { api, errorText } from "../lib/api";
import { BACKGROUNDS, IMAGE_SIZES, MODELS, OUTPUT_FORMATS } from "../lib/options";
import { gptRatioOptions, nanoRatioOptions } from "../components/Ratio";
import type { GenParams, ProxyMode, Settings, SettingsView } from "../lib/types";

const WEBSITE = "https://grsai.com/zh/models";
const NODES = [
  { value: "cn", label: "国内节点　grsai.dakka.com.cn" },
  { value: "global", label: "全球节点　grsaiapi.com" },
];
const PROXY_MODES = [
  { value: "none", label: "不使用" },
  { value: "system", label: "跟随系统" },
  { value: "custom", label: "自定义" },
];
const THEME_MODES = [
  { value: "light", label: "浅色" },
  { value: "dark", label: "深色" },
  { value: "system", label: "跟随系统" },
];
const COUNTS = [1, 2, 3, 4].map((n) => ({ value: n, label: n === 1 ? "1 张" : `${n} 合 1` }));

/** 主题字段由外观区即时保存，不进表单草稿 */
const withoutTheme = (s: Settings) => ({ ...s, themeAccent: "", themeMode: "" });

interface SettingsPageProps {
  view: SettingsView | null;
  onSaved: (view: SettingsView) => void;
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="flex flex-col gap-3 border-b pb-5 last:border-b-0 last:pb-0">
      <Typography.Title level={5} className="m-0">
        {title}
      </Typography.Title>
      {children}
    </section>
  );
}

export function SettingsPage({ view, onSaved }: SettingsPageProps) {
  // 变量名用 toast：App.useApp() 的 message，与静态 message 区分（也让 AI_GUIDE 自检的字面搜索通过）
  const { message: toast } = AntApp.useApp();
  const { logs, log, clear } = useLogger();
  const { accent, setAccent, mode, setMode } = useTheme();
  const [draft, setDraft] = useState<Settings | null>(null);
  const [saving, setSaving] = useState(false);
  const [testing, setTesting] = useState(false);

  useEffect(() => {
    if (view && !draft) setDraft(view.settings);
  }, [view, draft]);

  const dirty = useMemo(
    () => !!(view && draft && JSON.stringify(withoutTheme(view.settings)) !== JSON.stringify(withoutTheme(draft))),
    [view, draft]
  );

  if (!view || !draft) return <Page />;

  const patch = (p: Partial<Settings>) => setDraft((d) => (d ? { ...d, ...p } : d));
  const patchProxy = (p: Partial<Settings["proxy"]>) => setDraft((d) => (d ? { ...d, proxy: { ...d.proxy, ...p } } : d));
  const patchGpt = (p: Partial<GenParams>) => setDraft((d) => (d ? { ...d, gptParams: { ...d.gptParams, ...p } } : d));
  const patchNano = (p: Partial<GenParams>) => setDraft((d) => (d ? { ...d, nanoParams: { ...d.nanoParams, ...p } } : d));

  const save = async () => {
    setSaving(true);
    try {
      // 主题以当前生效的为准，避免草稿里的旧值覆盖
      const next = await api.saveSettings({ ...draft, themeAccent: view.settings.themeAccent, themeMode: view.settings.themeMode });
      onSaved(next);
      setDraft(next.settings);
      log("设置已保存。", "success");
      toast.success("设置已保存");
    } catch (err) {
      log(`保存失败：${errorText(err)}`, "error");
      toast.error(errorText(err));
    } finally {
      setSaving(false);
    }
  };

  const test = async () => {
    setTesting(true);
    log(`正在测试连接（${draft.node === "global" ? "全球" : "国内"}节点，不扣费）…`, "processing");
    try {
      const text = await api.testConnection(draft);
      log(text, "success");
      toast.success(text);
    } catch (err) {
      log(`测试失败：${errorText(err)}`, "error");
      toast.error(errorText(err));
    } finally {
      setTesting(false);
    }
  };

  const pickDir = async () => {
    const dir = await open({ directory: true, defaultPath: draft.outputDir || view.defaultOutputDir });
    if (typeof dir === "string") {
      patch({ outputDir: dir });
      log(`输出目录改为：${dir}（保存后生效）`);
    }
  };

  const shownDir = draft.outputDir || view.defaultOutputDir;
  const openDir = async () => {
    try {
      await api.openFolder(shownDir);
    } catch (err) {
      toast.error(errorText(err));
    }
  };

  const openWebsite = async () => {
    try {
      await openUrl(WEBSITE);
    } catch (err) {
      toast.error(errorText(err));
    }
  };

  const proxyOff = draft.proxy.mode === "none";

  return (
    <Page>
      <Workspace
        aside={
          <>
            <Panel title="关于绘境">
              <div className="flex items-center gap-3">
                <span className="grid size-10 shrink-0 place-items-center rounded-lg bg-primary text-text-light-solid">
                  <BrandMark />
                </span>
                <div className="min-w-0">
                  <div className="text-lg font-strong">绘境</div>
                  <div className="text-sm text-text-tertiary">AI 出图 · 文生图 / 图生图 / 继续修改</div>
                </div>
              </div>
              <Descriptions
                size="small"
                column={1}
                items={[
                  {
                    key: "version",
                    label: "版本",
                    children: (
                      <span className="flex items-center gap-2">
                        V{view.version}
                        {view.portable && <Tag variant="filled">便携版</Tag>}
                      </span>
                    ),
                  },
                  {
                    key: "data",
                    label: "数据目录",
                    children: (
                      <Typography.Text copyable={{ text: view.dataDir }} ellipsis={{ tooltip: view.dataDir }} className="max-w-[170px]">
                        {view.dataDir}
                      </Typography.Text>
                    ),
                  },
                  { key: "api", label: "接口", children: "GRSAI" },
                ]}
              />
              <Button icon={<GlobalOutlined />} onClick={openWebsite}>
                GRSAI 官网 · 模型与价格
              </Button>
            </Panel>
            <div className="mt-auto flex flex-col gap-2">
              {dirty && (
                <Button type="link" size="small" icon={<RollbackOutlined />} onClick={() => setDraft(view.settings)}>
                  放弃修改
                </Button>
              )}
              <Button type="primary" block loading={saving} disabled={!dirty} onClick={save}>
                保存设置
              </Button>
            </div>
          </>
        }
      >
        <Panel fill className="flex-1" title="设置" extra={<span className="text-sm text-text-tertiary">外观即时生效，其余点右下角「保存设置」</span>}>
          <div className="min-h-0 flex-1 overflow-y-auto pr-2">
            <Form layout="vertical" component="div" className="flex max-w-[640px] flex-col gap-5 [&_.ant-form-item]:mb-0">
              <Section title="接口">
                <Form.Item label="API Key" extra="在 GRSAI 控制台获取；「测试连接」不会扣费">
                  <Space.Compact block>
                    <Input.Password
                      value={draft.apiKey}
                      onChange={(e) => patch({ apiKey: e.target.value })}
                      placeholder="sk-…"
                      autoComplete="off"
                    />
                    <Button icon={<ApiOutlined />} loading={testing} disabled={!draft.apiKey.trim()} onClick={test}>
                      测试连接
                    </Button>
                  </Space.Compact>
                </Form.Item>
                <Form.Item label="节点">
                  <Radio.Group orientation="vertical" options={NODES} value={draft.node} onChange={(e) => patch({ node: e.target.value })} />
                </Form.Item>
              </Section>

              <Section title="网络代理">
                <Form.Item label="代理" extra="图片服务器走代理时可能握手失败，一般只给 API 请求用代理">
                  <Segmented options={PROXY_MODES} value={draft.proxy.mode} onChange={(v) => patchProxy({ mode: v as ProxyMode })} />
                </Form.Item>
                {draft.proxy.mode === "custom" && (
                  <Form.Item label="代理地址">
                    <Input
                      value={draft.proxy.url}
                      onChange={(e) => patchProxy({ url: e.target.value })}
                      placeholder="http://127.0.0.1:7890 或 socks5://127.0.0.1:7891"
                      status={draft.proxy.url && !/^(https?|socks5h?):\/\//.test(draft.proxy.url) ? "error" : undefined}
                    />
                  </Form.Item>
                )}
                <Form.Item label="用于">
                  <Space size="large">
                    <Checkbox checked={draft.proxy.forApi} disabled={proxyOff} onChange={(e) => patchProxy({ forApi: e.target.checked })}>
                      API 请求
                    </Checkbox>
                    <Checkbox checked={draft.proxy.forDownload} disabled={proxyOff} onChange={(e) => patchProxy({ forDownload: e.target.checked })}>
                      图片下载
                    </Checkbox>
                  </Space>
                </Form.Item>
              </Section>

              <Section title="输出">
                <Form.Item label="输出目录" extra={draft.outputDir ? undefined : "默认：图片/绘境"}>
                  <Space.Compact block>
                    <Input value={shownDir} readOnly />
                    <Button onClick={pickDir}>选择…</Button>
                  </Space.Compact>
                </Form.Item>
                <Space>
                  <Button icon={<FolderOpenOutlined />} onClick={openDir}>
                    {navigator.userAgent.includes("Mac") ? "在访达中打开" : "在资源管理器中打开"}
                  </Button>
                  <Button type="link" disabled={!draft.outputDir} onClick={() => patch({ outputDir: "" })}>
                    恢复默认
                  </Button>
                </Space>
                <Form.Item label="同时生成" extra="超出的任务排队等待，1–5">
                  <InputNumber min={1} max={5} precision={0} value={draft.concurrency} onChange={(v) => patch({ concurrency: v ?? 2 })} suffix="个" />
                </Form.Item>
              </Section>

              <Section title="默认参数">
                <div className="grid grid-cols-2 gap-3">
                  <Form.Item label="默认模型">
                    <Select value={draft.defaultModel} onChange={(v) => patch({ defaultModel: v })} options={MODELS} />
                  </Form.Item>
                  <Form.Item label="默认张数">
                    <Select value={draft.defaultCount} onChange={(v) => patch({ defaultCount: v })} options={COUNTS} />
                  </Form.Item>
                  <Form.Item label="GPT · 比例 / 尺寸">
                    <Select value={draft.gptParams.aspectRatio} onChange={(v) => patchGpt({ aspectRatio: v })} options={gptRatioOptions} />
                  </Form.Item>
                  <Form.Item label="GPT · 背景">
                    <Segmented block options={BACKGROUNDS} value={draft.gptParams.background} onChange={(v) => patchGpt({ background: String(v) })} />
                  </Form.Item>
                  <Form.Item label="GPT · 输出格式">
                    <Segmented
                      block
                      options={OUTPUT_FORMATS}
                      value={draft.gptParams.outputFormat || "png"}
                      onChange={(v) => patchGpt({ outputFormat: String(v) })}
                    />
                  </Form.Item>
                  <span />
                  <Form.Item label="Nano · 比例">
                    <Select value={draft.nanoParams.aspectRatio} onChange={(v) => patchNano({ aspectRatio: v })} options={nanoRatioOptions} />
                  </Form.Item>
                  <Form.Item label="Nano · 分辨率">
                    <Segmented block options={IMAGE_SIZES} value={draft.nanoParams.imageSize || "1K"} onChange={(v) => patchNano({ imageSize: String(v) })} />
                  </Form.Item>
                </div>
              </Section>

              <Section title="多合一">
                <Form.Item extra="多合一拆切后，把每张缩放到该模型单独出一张时的尺寸（Lanczos3 算法）；关闭则保留拆切出的原始尺寸">
                  <Checkbox checked={draft.upscaleParts} onChange={(e) => patch({ upscaleParts: e.target.checked })}>
                    拆切后放大到单张原尺寸
                  </Checkbox>
                </Form.Item>
              </Section>

              <Section title="外观">
                <Form.Item label="明暗">
                  <Segmented options={THEME_MODES} value={mode} onChange={(v) => setMode(v as ThemeMode)} />
                </Form.Item>
                <Form.Item label="主题">
                  <ThemePicker value={accent} onChange={setAccent} />
                </Form.Item>
              </Section>
            </Form>
          </div>
        </Panel>
      </Workspace>

      <Panel
        fill
        title="设置日志"
        size="small"
        className="h-[170px] shrink-0"
        extra={
          <Button type="link" size="small" disabled={!logs.length} onClick={clear}>
            清空日志
          </Button>
        }
      >
        <LogViewer logs={logs} emptyText="测试连接、保存设置的结果会显示在这里" />
      </Panel>
    </Page>
  );
}
