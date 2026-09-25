import type { PaginationProps } from 'naive-ui'
import type { AppColorTokens } from '@shared/utils/colorScheme'

export function buildTaskPaginationTheme(
  tokens: AppColorTokens,
  opacityPercent: string,
): NonNullable<PaginationProps['themeOverrides']> {
  const translucent = (color: string) => `color-mix(in srgb, ${color} ${opacityPercent}, transparent)`
  const border = (color: string) => `1px solid ${translucent(color)}`

  return {
    buttonBorder: border(tokens.outlineVariant),
    buttonBorderHover: border(tokens.outline),
    buttonBorderPressed: border(tokens.outline),
    itemBorderActive: border(tokens.primary.color),
    itemBorderDisabled: border(tokens.outlineVariant),
    itemColorDisabled: translucent(tokens.surfaceContainerLow),
    peers: {
      Select: {
        peers: {
          InternalSelection: {
            color: translucent(tokens.surfaceContainer),
            colorActive: translucent(tokens.surfaceContainer),
            colorDisabled: translucent(tokens.surfaceContainerLow),
            border: border(tokens.outlineVariant),
            borderHover: border(tokens.outline),
            borderFocus: border(tokens.primary.color),
            borderActive: border(tokens.primary.color),
          },
        },
      },
    },
  }
}
