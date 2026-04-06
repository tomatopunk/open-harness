# PLUGINS DIRECTORY

## OVERVIEW

Plugin directory containing gateway-plugin, manage-plugin, and dingtalk-plugin.

## STRUCTURE

```
plugins/
├── gateway-plugin/     # API gateway plugin (OpenAI compatible API)
├── manage-plugin/      # Management plugin
└── dingtalk-plugin/    # DingTalk plugin
```

## WHERE TO LOOK

| Task | Location |
|------|----------|
| API gateway | gateway-plugin/ |
| Management features | manage-plugin/ |
| DingTalk integration | dingtalk-plugin/ |

## PLUGIN DEVELOPMENT

Steps to create a plugin:
1. Create plugin directory in `plugins/`
2. Create `plugin.yaml` manifest
3. Implement `Plugin` trait
