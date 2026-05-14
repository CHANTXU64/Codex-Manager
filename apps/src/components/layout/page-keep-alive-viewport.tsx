"use client";

import {
  lazy,
  Suspense,
  useEffect,
  useMemo,
  useState,
  type ComponentType,
  type LazyExoticComponent,
  type ReactNode,
} from "react";
import { Loader2 } from "lucide-react";
import { usePathname } from "next/navigation";
import {
  type TopLevelRoutePath,
  type TopLevelRouteAccessContext,
  getAllowedTopLevelRoutes,
  getFirstAllowedTopLevelRoutePath,
  getTopLevelRouteLabel,
  isTopLevelRouteAllowedForRole,
  toTopLevelRoutePath,
} from "@/lib/app-shell/top-level-routes";
import { useAppSession } from "@/hooks/useAppSession";
import { useI18n } from "@/lib/i18n/provider";
import { useAppStore } from "@/lib/store/useAppStore";
import { cn } from "@/lib/utils";

const ROOT_ROUTE_PATH = "/";

const LAZY_PAGE_COMPONENTS: Record<
  Exclude<TopLevelRoutePath, typeof ROOT_ROUTE_PATH>,
  LazyExoticComponent<ComponentType>
> = {
  "/accounts": lazy(() => import("@/app/accounts/page")),
  "/account-manager": lazy(() => import("@/app/account-manager/page")),
  "/aggregate-api": lazy(() => import("@/app/aggregate-api/page")),
  "/apikeys": lazy(() => import("@/app/apikeys/page")),
  "/models": lazy(() => import("@/app/models/page")),
  "/model-groups": lazy(() => import("@/app/model-groups/page")),
  "/plugins": lazy(() => import("@/app/plugins/page")),
  "/logs": lazy(() => import("@/app/logs/page")),
  "/settings": lazy(() => import("@/app/settings/page")),
  "/author": lazy(() => import("@/app/author/page")),
};

const ROOT_PAGE_COMPONENT = lazy(() => import("@/app/page"));

function PagePanelFallback({ title }: { title: string }) {
  const isSidebarOpen = useAppStore((state) => state.isSidebarOpen);

  return (
    <div
      className={cn(
        "fixed inset-y-0 right-0 z-40 overflow-hidden bg-white/28 backdrop-blur-md",
        isSidebarOpen ? "left-56" : "left-16",
      )}
    >
      <div className="absolute inset-0 bg-[radial-gradient(circle_at_top,_rgba(168,85,247,0.14),_transparent_42%)]" />
      <div className="absolute inset-0 bg-[linear-gradient(180deg,rgba(255,255,255,0.16)_0%,rgba(255,255,255,0.04)_24%,rgba(255,255,255,0.24)_100%)]" />
      <div className="relative flex h-full w-full items-start justify-center px-8 pt-[31vh]">
        <div className="flex w-full max-w-2xl flex-col items-center gap-5 text-center">
          <div className="flex h-20 w-20 items-center justify-center rounded-full bg-background/55 text-primary shadow-[0_18px_50px_rgba(168,85,247,0.16)] ring-1 ring-white/45 backdrop-blur-sm">
            <Loader2 className="h-10 w-10 animate-spin" />
          </div>
          <div className="space-y-2">
            <p className="text-2xl font-semibold tracking-tight text-foreground/95">{title}</p>
            <p className="text-sm text-muted-foreground">正在恢复页面内容，请稍候...</p>
          </div>
          <div className="h-2.5 w-full max-w-xl overflow-hidden rounded-full bg-white/45 shadow-[inset_0_1px_2px_rgba(15,23,42,0.08)]">
            <div className="h-full w-2/5 animate-pulse rounded-full bg-primary/70" />
          </div>
          <div className="inline-flex items-center gap-2 rounded-full bg-background/45 px-3 py-1.5 text-xs text-muted-foreground shadow-sm ring-1 ring-white/40 backdrop-blur-sm">
            <span className="h-2 w-2 rounded-full bg-primary/75" />
            <span>页面缓存已命中，正在恢复视图与数据状态</span>
          </div>
        </div>
      </div>
    </div>
  );
}

function LazyPagePanel({
  path,
  access,
}: {
  path: TopLevelRoutePath;
  access: TopLevelRouteAccessContext;
}) {
  const LazyPage = path === ROOT_ROUTE_PATH ? ROOT_PAGE_COMPONENT : LAZY_PAGE_COMPONENTS[path];

  return (
    <Suspense fallback={<PagePanelFallback title={getTopLevelRouteLabel(path, access)} />}>
      <LazyPage />
    </Suspense>
  );
}

export function PageKeepAliveViewport({
  initialChildren,
}: {
  initialChildren: ReactNode;
}) {
  const { t } = useI18n();
  const pathname = usePathname();
  const [normalizedInitialPath] = useState<TopLevelRoutePath>(() =>
    toTopLevelRoutePath(pathname),
  );
  const currentShellPath = useAppStore((state) => state.currentShellPath);
  const openShellTabs = useAppStore((state) => state.openShellTabs);
  const syncShellPathFromLocation = useAppStore(
    (state) => state.syncShellPathFromLocation,
  );
  const pruneShellTabs = useAppStore((state) => state.pruneShellTabs);
  const { data: session, isLoading: isSessionLoading } = useAppSession();
  const role = session?.role ?? "member";
  const routeAccess = useMemo(
    () => ({ role, mode: session?.mode ?? null }),
    [role, session?.mode],
  );

  useEffect(() => {
    syncShellPathFromLocation(normalizedInitialPath);
  }, [normalizedInitialPath, syncShellPathFromLocation]);

  useEffect(() => {
    const handlePopState = () => {
      syncShellPathFromLocation(window.location.pathname);
    };

    window.addEventListener("popstate", handlePopState);
    return () => {
      window.removeEventListener("popstate", handlePopState);
    };
  }, [syncShellPathFromLocation]);

  useEffect(() => {
    document.title = `${t(getTopLevelRouteLabel(currentShellPath, routeAccess))} - CodexManager`;
  }, [currentShellPath, routeAccess, t]);

  useEffect(() => {
    if (isSessionLoading) return;
    const allowedPaths = getAllowedTopLevelRoutes(routeAccess).map((route) => route.path);
    pruneShellTabs(allowedPaths, getFirstAllowedTopLevelRoutePath(routeAccess));
  }, [isSessionLoading, pruneShellTabs, routeAccess]);

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="relative min-h-0 flex-1">
        {openShellTabs.map((path) => {
          if (!isTopLevelRouteAllowedForRole(path, routeAccess)) {
            return null;
          }
          const isActive = path === currentShellPath;
          const isInitialPanel = path === normalizedInitialPath;

          return (
            <section
              key={path}
              aria-hidden={!isActive}
              data-shell-path={path}
              className={cn(
                "relative min-h-[calc(100vh-11rem)]",
                isActive ? "block" : "hidden",
              )}
            >
              {isInitialPanel ? initialChildren : <LazyPagePanel path={path} access={routeAccess} />}
            </section>
          );
        })}
      </div>
    </div>
  );
}
