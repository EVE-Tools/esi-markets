docker run -t -i --rm -v $PWD:/volume -t clux/muslrust cargo build --release
docker build -f ./Dockerfile-static -t evetools/esi-markets:latest .
