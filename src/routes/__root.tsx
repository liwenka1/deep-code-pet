import { QueryClientProvider, type QueryClient } from '@tanstack/react-query'
import type { ReactNode } from 'react'
import { ClientOnly, createRootRouteWithContext, HeadContent, Outlet, Scripts } from '@tanstack/react-router'
import baseStyles from '../shared/styles/base.css?url'

export const Route = createRootRouteWithContext<{ queryClient: QueryClient }>()({
  head: () => ({
    meta: [
      { charSet: 'utf-8' },
      { name: 'viewport', content: 'width=device-width, initial-scale=1' },
      { title: 'deep-code-pet' },
    ],
    links: [{ rel: 'stylesheet', href: baseStyles }],
  }),
  component: Root,
  shellComponent: RootDocument,
  notFoundComponent: () => <p role="alert">找不到这个页面</p>,
})

function Root() {
  const { queryClient } = Route.useRouteContext()
  // Native APIs and window-specific UI start only after the static shell hydrates.
  return <QueryClientProvider client={queryClient}><ClientOnly><Outlet /></ClientOnly></QueryClientProvider>
}

function RootDocument({ children }: { children: ReactNode }) {
  return (
    <html lang="zh-CN">
      <head><HeadContent /></head>
      <body>
        {children}
        <Scripts />
      </body>
    </html>
  )
}
