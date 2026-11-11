(function () {
  var frame = { ready: false, failed: false };
  var views = [];
  var halvo = window.Halvo || {};
  window.__r4frame = frame;
  halvo.views = {
    add: function (view) {
      views.push(view);
    },
    window: function (state) {
      views.forEach(function (view) {
        if (state === 'shown') view.show();
        else view.hide();
      });
    }
  };
  if (!halvo.ipc) {
    halvo.ipc = {
      call: function () {
        return Promise.reject(new Error('no core'));
      }
    };
  }
  window.Halvo = halvo;

  function load(urls, i, done) {
    if (i >= urls.length) {
      done();
      return;
    }
    var script = document.createElement('script');
    script.src = urls[i];
    script.onload = function () {
      load(urls, i + 1, done);
    };
    script.onerror = function () {
      frame.failed = true;
      load(urls, i + 1, done);
    };
    document.head.appendChild(script);
  }

  function start() {
    var list = new URLSearchParams(location.search).get('views');
    var urls = list ? list.split(',').filter(Boolean) : [];
    load(urls, 0, function () {
      var main = document.getElementById('views');
      views.slice().sort(function (a, b) {
        return a.order - b.order;
      }).forEach(function (view) {
        var section = document.createElement('section');
        var heading = document.createElement('h2');
        var body = document.createElement('div');
        heading.id = 'view-' + view.id;
        heading.textContent = view.heading;
        section.setAttribute('aria-labelledby', heading.id);
        section.appendChild(heading);
        section.appendChild(body);
        main.appendChild(section);
        view.mount(body);
      });
      views.forEach(function (view) {
        view.show();
      });
      frame.ready = true;
    });
  }

  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', start);
  else start();
})();
