import java.io.IOException;
import java.net.InetAddress;
import java.net.ServerSocket;
import java.net.Socket;
import java.rmi.server.RMISocketFactory;

public class LoopbackTlc {
    public static void main(String[] args) throws Exception {
        RMISocketFactory.setSocketFactory(new RMISocketFactory() {
            @Override
            public Socket createSocket(String host, int port) throws IOException {
                return new Socket(InetAddress.getLoopbackAddress(), port);
            }

            @Override
            public ServerSocket createServerSocket(int port) throws IOException {
                return new ServerSocket(port, 0, InetAddress.getLoopbackAddress());
            }
        });
        tlc2.TLC.main(args);
    }
}
