
## Install Docker on EC2 Ubuntu

To install Docker on an Ubuntu server hosted on AWS, you typically follow these steps:

1. Update your existing list of packages:
   ```bash
   sudo apt update
   ```

2. Install prerequisite packages which let `apt` use packages over HTTPS:
   ```bash
   sudo apt install apt-transport-https ca-certificates curl software-properties-common
   ```

3. Add the GPG key for the official Docker repository to your system:
   ```bash
   curl -fsSL https://download.docker.com/linux/ubuntu/gpg | sudo apt-key add -
   ```

4. Add the Docker repository to APT sources:
   ```bash
   sudo add-apt-repository "deb [arch=amd64] https://download.docker.com/linux/ubuntu $(lsb_release -cs) stable"
   ```

5. Update the package database with the Docker packages from the newly added repo:
   ```bash
   sudo apt update
   ```

6. Make sure you are about to install from the Docker repo instead of the default Ubuntu repo:
   ```bash
   apt-cache policy docker-ce
   ```

7. Finally, install Docker:
   ```bash
   sudo apt install docker-ce
   ```

8. Check that Docker is running:
   ```bash
   sudo systemctl status docker
   ```

Remember to run these commands with `sudo` if you are not logged in as the root user.


## Helpful Docker Commands

Be careful when running these commands. They can be destructive.

1. **Add your user to the Docker group**: This lets your user run Docker commands without `sudo`. Run the following command to add your user to the Docker group:
   ```bash
   sudo usermod -aG docker ${USER}
   ```
2. **Remove all containers in Docker**: 
   ```bash
   docker rm -f $(sudo docker ps -aq)
   ```

3. **Restart the Docker service**: Sometimes, the Docker daemon might be in a state that prevents access. Restarting it can resolve the issue:
   ```bash
   sudo systemctl restart docker
   ```
4. **Clean up everything except volumes**: 
   ```bash
   docker system prune -af
   ```
5. **Remove volumes**:
   ```bash
   docker system prune -af --volumes
   ```


## To build a multi-architecture Docker image

Docker's buildx tool, which is a part of Docker BuildKit. This tool allows you to create images for different platforms from a single build command. Here's a basic rundown of the steps:

1. **Enable experimental features** by setting `"experimental": "enabled"` in your Docker configuration file (`~/.docker/config.json`).

2. **Install QEMU** on your macOS to emulate different architectures:
   ```sh
   brew install qemu
   ```

3. **Create a new builder** that enables buildx and specify the platforms you want to target:
   ```sh
   docker buildx create --name mybuilder --use
   ```

4. **Inspect the builder** to ensure it's correctly configured and can build for the target platforms:
   ```sh
   docker buildx inspect mybuilder --bootstrap
   ```

5. **Build and push the image** to Docker Hub (or another registry) for the desired platforms using the `--platform` flag:
   ```sh
   docker buildx build --platform linux/amd64,linux/arm64,linux/arm/v7 -t <username>/myimage:latest --push .
   ```