import { createHashRouter, RouterProvider } from 'react-router';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { ThemeProvider } from './providers/ThemeProvider';
import { ServiceOperationsProvider } from './providers/ServiceOperationsProvider';
import { RemovingProjectsProvider } from './providers/RemovingProjectsProvider';
import Layout from './components/Layout';
import ProjectsPage from './pages/ProjectsPage';
import ArchivedProjectsPage from './pages/ArchivedProjectsPage';
import ProjectDetailPage from './pages/ProjectDetailPage';
import InstanceDetailPage from './pages/InstanceDetailPage';
import ServiceDetailPage from './pages/ServiceDetailPage';
import ImageDetailPage from './pages/ImageDetailPage';
import VolumeDetailPage from './pages/VolumeDetailPage';
import HostServiceDetailPage from './pages/HostServiceDetailPage';
import HostImageDetailPage from './pages/HostImageDetailPage';
import BuildDetailPage from './pages/BuildDetailPage';
import BareServiceDetailPage from './pages/BareServiceDetailPage';
import RemoteDetailPage from './pages/RemoteDetailPage';
import RemoteInstanceDetailPage from './pages/RemoteInstanceDetailPage';
import RemoteServiceDetailPage from './pages/RemoteServiceDetailPage';
import DocsPage from './pages/DocsPage';

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 5_000,
      retry: 1,
    },
  },
});

const router = createHashRouter([
  {
    element: <Layout />,
    children: [
      { index: true, element: <ProjectsPage /> },
      { path: 'archived', element: <ArchivedProjectsPage /> },
      { path: 'project/:project/host-services/:service/:tab', element: <HostServiceDetailPage /> },
      { path: 'project/:project/host-services/:service', element: <HostServiceDetailPage /> },
      { path: 'project/:project/host-images/:imageId', element: <HostImageDetailPage /> },
      { path: 'project/:project/builds/:buildId', element: <BuildDetailPage /> },
      { path: 'project/:project/remotes/:remote/:tab', element: <RemoteDetailPage /> },
      { path: 'project/:project/remotes/:remote', element: <RemoteDetailPage /> },
      { path: 'project/:project/:tab', element: <ProjectDetailPage /> },
      { path: 'project/:project', element: <ProjectDetailPage /> },
      { path: 'remote-instance/:project/:name/services/:service/:tab', element: <RemoteServiceDetailPage /> },
      { path: 'remote-instance/:project/:name/services/:service', element: <RemoteServiceDetailPage /> },
      { path: 'remote-instance/:project/:name/images/:imageId', element: <ImageDetailPage /> },
      { path: 'remote-instance/:project/:name/volumes/:volumeName', element: <VolumeDetailPage /> },
      { path: 'remote-instance/:project/:name/:tab', element: <RemoteInstanceDetailPage /> },
      { path: 'remote-instance/:project/:name', element: <RemoteInstanceDetailPage /> },
      { path: 'instance/:project/:name/services/:service/:tab', element: <ServiceDetailPage /> },
      { path: 'instance/:project/:name/services/:service', element: <ServiceDetailPage /> },
      { path: 'instance/:project/:name/bare-services/:service', element: <BareServiceDetailPage /> },
      { path: 'instance/:project/:name/images/:imageId', element: <ImageDetailPage /> },
      { path: 'instance/:project/:name/volumes/:volumeName', element: <VolumeDetailPage /> },
      { path: 'instance/:project/:name/:tab', element: <InstanceDetailPage /> },
      { path: 'instance/:project/:name', element: <InstanceDetailPage /> },
      { path: 'docs', element: <DocsPage /> },
      { path: 'docs/*', element: <DocsPage /> },
    ],
  },
]);

export default function App() {
  return (
    <ThemeProvider>
      <QueryClientProvider client={queryClient}>
        <ServiceOperationsProvider>
          <RemovingProjectsProvider>
            <RouterProvider router={router} />
          </RemovingProjectsProvider>
        </ServiceOperationsProvider>
      </QueryClientProvider>
    </ThemeProvider>
  );
}
