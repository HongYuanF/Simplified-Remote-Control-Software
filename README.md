## Build

Need: cmake、git


## 构建

在windows上面安装cmake,rust工具链，具体请上官网下载。
在根目录运行命令`cargo build --release`，我们在target\release文件夹下发现server.exe和client.exe。
在一个电脑上运行server.exe，如果防火墙询问你，你就同意。
另一个电脑上运行client.exe，但是client需要输入正确的server的地址才能知道，这个需要server和client在同一个局域网下，ip地址需要在服务端的windows上通过ipconfig获得。
打开就能看到了。
