```bash
sudo apt install debootstrap

mkdir -p rootfs

sudo debootstrap --variant=minbase jammy ./rootfs http://archive.ubuntu.com/ubuntu/


sudo ./target/release/cor run /bin/bash
```