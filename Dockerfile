FROM psychonaut/rust-nightly:latest

ADD . /my-source

RUN cd /my-source && cargo build -v
#--release

CMD ["/my-source/target/debug/eye_of_providence"]
